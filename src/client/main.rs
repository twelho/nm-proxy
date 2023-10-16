// (c) Dennis Marttinen 2023
// SPDX-License-Identifier: GPL-3.0-or-later

use std::env;
use std::os::unix::prelude::*;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use tokio::fs::read_dir;
use tokio::io::copy;
use tokio::net::UnixStream;
use tokio::signal;
use tokio::task::JoinSet;
use tokio_fd::AsyncFd;

use nm_proxy::common;

async fn parse_args() -> Result<(String, Vec<String>)> {
    let mut args = env::args().collect::<Vec<_>>().into_iter();
    let invocation_path = args
        .next()
        .ok_or(anyhow!("Unable to acquire invocation path"))?;

    if let (Some(manifest_path), Some(app_id), None) = (args.next(), args.next(), args.next()) {
        if let Some(file_name) = PathBuf::from(&manifest_path).file_name() {
            let manifest_name = file_name
                .to_os_string()
                .into_string()
                .map_err(|s| anyhow!("Failed to parse file name {:?}", s))?;
            return Ok((manifest_name, vec![manifest_path, app_id]));
        }
    }

    bail!(
        "Usage: {} <app-manifest-path> <extension-id>\n\
        This binary should be invoked by a browser through native messaging.",
        invocation_path
    )
}

async fn find_socket() -> Result<String> {
    let runtime_dir = common::parse_env("XDG_RUNTIME_DIR", None)?;

    let mut stream = read_dir(&runtime_dir)
        .await
        .with_context(|| format!("Failed to read {}", runtime_dir))?;

    while let Some(entry) = stream
        .next_entry()
        .await
        .with_context(|| format!("Failed to access entry in {}", runtime_dir))?
    {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|s| anyhow!("Failed to parse file name {:?}", s))?;

        //eprintln!("Parsing file: {}, type: {:?}", name, entry.file_type().await?);
        // TODO: is_socket() does not work in Flatpak, bug in Rust? Debug information:
        //  Inside Flatpak: FileType(FileType { mode: 32768 })
        //  On host system: FileType(FileType { mode: 49152 })

        // Pick the first file with a matching name, testing for sockets
        // with is_socket() does not work in a Flatpak for some reason
        if entry
            .file_type()
            .await
            .with_context(|| format!("Failed to read type of {}", name))?
            .is_file()
            && name.starts_with(common::SOCKET_PREFIX)
        {
            return Ok(entry
                .path()
                .into_os_string()
                .into_string()
                .map_err(|s| anyhow!("Failed to parse path {:?}", s))?);
        }
    }

    bail!("No valid socket found in {}", runtime_dir)
}

#[tokio::main]
async fn main() -> Result<()> {
    let (manifest_name, args) = parse_args().await?;
    let socket_path = find_socket().await?;

    // Connect to the socket
    let stream = UnixStream::connect(&socket_path)
        .await
        .with_context(|| format!("Failed to connect to socket {}", socket_path))?;

    // Split the socket stream into RX/TX
    let (mut socket_rx, mut socket_tx) = stream.into_split();

    let mut stdin =
        AsyncFd::try_from(libc::STDIN_FILENO).context("Unable to asynchronously open stdin")?;
    let mut stdout =
        AsyncFd::try_from(libc::STDOUT_FILENO).context("Unable to asynchronously open stdout")?;

    // Construct and send handshake message to the socket
    common::send_nm_object(
        &mut socket_tx,
        common::HandshakeMessage {
            manifest_name,
            args,
        },
    )
    .await
    .context("Sending handshake message failed")?;

    // Spawn bidirectional asynchronous copy tasks
    let mut set = JoinSet::new();
    set.spawn(async move { copy(&mut stdin, &mut socket_tx).await.map(|_| false) });
    set.spawn(async move { copy(&mut socket_rx, &mut stdout).await.map(|_| false) });

    // Graceful shutdown helper task
    set.spawn(async move { signal::ctrl_c().await.map(|_| true) });

    // Wait for any one of the tasks and then abort all others
    let mut aborted = false;
    let mut graceful = false;
    while let Some(result) = set.join_next().await {
        match result {
            Ok(Ok(signal)) => graceful |= signal,
            Ok(Err(e)) => Err(e).context("Task encountered error")?,
            Err(e) if e.is_cancelled() => (), // Cancellations are expected
            Err(e) => Err(e).context("Unexpected error when joining task")?,
        }

        // Abort all tasks after first task has finished
        if !aborted {
            aborted = true;
            set.abort_all();
        }
    }

    if graceful {
        Ok(())
    } else {
        bail!("Unclean shutdown, did the socket close?")
    }
}
