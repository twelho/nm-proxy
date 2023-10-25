// (c) Dennis Marttinen 2023
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{anyhow, Context, Result};
use libc::pid_t;
use nix::sys::signal;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use std::collections::HashMap;
use std::fmt::Debug;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{copy, AsyncBufReadExt, AsyncRead, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::select;
use tokio::task::JoinSet;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, warn};

use nm_proxy::common::{recv_nm_object, HandshakeMessage};

pub struct ClientTaskConfig {
    pub browser: String,
    pub stream: UnixStream,
    pub bin_map: Arc<HashMap<String, String>>,
    pub token: CancellationToken,
}

impl ClientTaskConfig {
    #[instrument(skip_all, fields(id = _id, browser = self.browser, manifest), err)]
    pub(crate) async fn launch(self, _id: u32) -> Result<()> {
        info!("waiting for handshake");
        let (mut stream_rx, mut stream_tx) = self.stream.into_split();
        let handshake: HandshakeMessage = recv_nm_object(&mut stream_rx)
            .await
            .context("Receiving handshake message failed")?;

        // Register the manifest name into the instrumentation
        tracing::Span::current().record("manifest", &handshake.manifest_name);
        info!("client connected");

        let binary = self.bin_map.get(&handshake.manifest_name).ok_or(anyhow!(
            "Native binary for {} not registered",
            handshake.manifest_name
        ))?;

        info!("launching native binary: {}", binary);
        debug!("handshake args: {:?}", handshake.args);

        // Start the native binary as a subprocess
        let mut child = Command::new(binary)
            .args(handshake.args) // Pass through the arguments from the browser
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let mut child_stdin = child.stdin.take().unwrap();
        let mut child_stdout = child.stdout.take().unwrap();

        let child_stderr = child.stderr.take().unwrap();
        let binary_clone = binary.clone();

        // This will abort all nested tasks when dropped
        let mut set = JoinSet::new();
        set.spawn(async move { copy(&mut child_stdout, &mut stream_tx).await.map(|_| ()) });
        set.spawn(async move { copy(&mut stream_rx, &mut child_stdin).await.map(|_| ()) });
        set.spawn(
            async move { stderr_task(child_stderr, _id, &self.browser, &*binary_clone).await },
        );

        // Dummy task for triggering cancellation
        set.spawn(async move {
            self.token.cancelled().await;
            Ok(())
        });

        let mut aborted = false;
        while let Some(a) = set.join_next().await {
            match a {
                Ok(Ok(_)) => (),
                Ok(Err(e)) => Err(e).context("IO task error")?,
                Err(e) if e.is_cancelled() => (), // Cancellations are expected
                Err(e) => Err(e).context("IO task join failed")?,
            }

            if !aborted {
                aborted = true;

                // Give the application a little time to react to stdio being closed
                time::sleep(time::Duration::from_millis(200)).await;

                // Send SIGTERM to native binary (regardless of task that quit)
                if let Some(id) = child.id() {
                    signal::kill(Pid::from_raw(id as pid_t), Signal::SIGTERM).unwrap();
                }

                // Wait for 10 seconds for the process to quit
                select! {
                    _ = time::sleep(time::Duration::from_secs(10)) => {
                        warn!("timeout reached, killing {binary}");
                        child.kill().await.with_context(|| format!("failed to kill {binary}"))?;
                    }
                    res = child.wait() => {
                        match res {
                            Ok(s) => info!("{binary}: {s}"),
                            Err(e) => Err(e)
                                .with_context(|| format!("{binary}: unclean shutdown"))?
                        };
                    }
                }

                // Abort all IO tasks after first task has finished
                set.abort_all();
            }
        }

        Ok(())
    }
}

/// Prints warning messages from stderr of a child process
#[instrument(skip_all, fields(id = _id, browser = _browser, binary = _binary))]
async fn stderr_task(
    stderr: impl AsyncRead + Unpin + Debug,
    _id: u32,
    _browser: &str,
    _binary: &str,
) -> std::io::Result<()> {
    let mut buf = String::new();
    let mut reader = BufReader::new(stderr);

    loop {
        match reader.read_line(&mut buf).await {
            Ok(0) => return Ok(()), // Closed
            Ok(_) => {
                buf.pop();
                // Remove newline
                warn!("task error: {}", buf);
                buf.clear(); // Clear buffer for next message
            }
            Err(e) => return Err(e),
        }
    }
}
