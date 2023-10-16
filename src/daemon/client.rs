// (c) Dennis Marttinen 2023
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::io::{BufRead, Read};
use std::os::fd::{AsFd, AsRawFd};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::task::Poll;

use anyhow::{anyhow, Context, Error, Result};
use log::{debug, error, info, warn};
use nm_proxy::common::{recv_nm_object, HandshakeMessage};
use tokio::io::{copy, AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{ChildStderr, Command};
use tokio::task::JoinSet;
use tokio_fd::AsyncFd;

pub struct ClientTaskConfig {
    pub stream: UnixStream,
    pub bin_map: Arc<HashMap<String, String>>,
}

impl ClientTaskConfig {
    pub async fn launch(self) {
        match launch_tasks(self).await {
            Ok(_) => info!("task complete"),
            Err(e) => error!("task crash:\n{:?}", e),
        };
    }
}

async fn launch_tasks(mut config: ClientTaskConfig) -> Result<()> {
    let (mut stream_rx, mut stream_tx) = config.stream.into_split();
    let handshake: HandshakeMessage = recv_nm_object(&mut stream_rx)
        .await
        .context("Receiving handshake message failed")?;

    info!("client for {} connected", handshake.manifest_name);
    debug!("handshake args: {:?}", handshake.args);

    let binary = config.bin_map.get(&handshake.manifest_name).ok_or(anyhow!(
        "Native binary for {} not registered",
        handshake.manifest_name
    ))?;

    // Start your target application as a subprocess
    let mut child = Command::new(binary)
        .args(handshake.args) // Pass through the arguments from the browser
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let mut child_stdin = child.stdin.take().unwrap();
    let mut child_stdout = child.stdout.take().unwrap();
    let mut child_stderr = child.stderr.take().unwrap();

    // This will abort all nested tasks when dropped
    let mut set = JoinSet::new();
    set.spawn(async move { copy(&mut child_stdout, &mut stream_tx).await.map(|_| ()) });
    set.spawn(async move { copy(&mut stream_rx, &mut child_stdin).await.map(|_| ()) });
    set.spawn(async move { stderr_task(child_stderr).await.map(|_| ()) });

    // TODO: Process the JoinSet output
    // TODO: Handle SIGTERM
    while let Some(_) = set.join_next().await {}

    Ok(())
}

/// Prints warning messages from stderr of child process
async fn stderr_task(mut stderr: impl AsyncRead + Unpin) -> std::io::Result<()> {
    let mut buf = String::new();
    let mut reader = BufReader::new(stderr);

    loop {
        match reader.read_line(&mut buf).await {
            Ok(0) => return Ok(()), // Closed
            Ok(_) => {
                buf.pop(); // Remove newline
                warn!("task error: {}", buf);
                buf.clear(); // Clear buffer for next message
            }
            Err(e) => return Err(e),
        }
    }
}
