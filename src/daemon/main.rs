// (c) Dennis Marttinen 2023
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::client::ClientTaskConfig;
use anyhow::{anyhow, bail, Context, Error, Result};
use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::os::unix::net as std_net;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::task::JoinSet;
use tokio::{select, signal};
use tokio_util::sync::CancellationToken;
use tracing::instrument;
use tracing::{error, info};

use nm_proxy::common;
use nm_proxy::common::runtime::Settings;
use nm_proxy::common::traits::*;

mod client;

#[instrument(level = "debug", ret)]
async fn parse_sockets() -> Result<HashMap<String, OwnedFd>> {
    Ok(sd_listen_fds::get()
        .context("Socket parsing failed")?
        .into_iter()
        .map(|(name, fd)| {
            let std_fd = fd.into_std();
            match name {
                None => Err(anyhow!("No name provided for fd {:?}", std_fd)),
                Some(n) => Ok((n, std_fd)),
            }
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .collect())
}

struct ListenerConfig {
    browser: String,
    listener: UnixListener,
    bin_map_arc: Arc<HashMap<String, String>>,
    task_id_gen: Arc<AtomicU32>,
    token: CancellationToken,
}

impl ListenerConfig {
    #[instrument(skip_all, fields(browser = self.browser))]
    async fn spawn_listener(self) -> Result<()> {
        info!("listening for incoming native messaging connections");

        // This will abort all nested tasks when dropped
        let mut client_set = JoinSet::new();

        loop {
            select! {
                _ = self.token.cancelled() => { break }
                res = self.listener.accept() => {
                    match res {
                        Ok((stream, _)) => {
                            let browser = self.browser.clone();
                            let bin_map = self.bin_map_arc.clone();
                            let id = self.task_id_gen.fetch_add(1, Ordering::Relaxed);
                            let token = self.token.clone();
                            client_set.spawn(async move {
                                let res = ClientTaskConfig {
                                    browser,
                                    stream,
                                    bin_map,
                                    token,
                                }
                                .launch(id)
                                .await;
                                res
                            });
                        }
                        Err(e) => {
                            error!("error accepting client: {e}");
                        }
                    }
                }
            }
        }

        while let Some(result) = client_set.join_next().await {
            match result {
                Ok(Ok(_)) => (),
                Ok(Err(e)) => Err(e).context("client task error")?,
                Err(e) => Err(e).context("client task join failed")?,
            }
        }

        Ok(())
    }
}

#[tokio::main]
#[instrument]
async fn main() -> Result<()> {
    // Initialize the logging framework
    tracing_subscriber::fmt::init();

    // Parse sockets passed by systemd
    let mut sockets = parse_sockets().await?;
    if sockets.is_empty() {
        bail!("The daemon must be launched as a systemd socket-activated service");
    }

    // Acquire the runtime directory path
    let runtime_dir = common::parse_env("XDG_RUNTIME_DIR", None)?;

    // Load runtime settings
    let settings = Settings::load(&runtime_dir).await?;

    let mut set = JoinSet::new();
    let task_id = Arc::new(AtomicU32::new(0));
    let token = CancellationToken::new();

    for (browser, bin_map) in settings.native_binaries {
        // Retrieve fd from socket configuration
        let fd = match sockets.remove(&browser) {
            Some(fd) => fd,
            None => {
                return Err(anyhow!("{}: socket not found", browser).context(
                    r"
Expected socket from systemd, but it is absent. Check
ListenStream/FileDescriptorName entries in socket unit(s)",
                ));
            }
        };

        // Construct UNIX socket listener
        let listener =
            UnixListener::from_std(std_net::UnixListener::from(fd)).path_context(&browser)?;

        // These need to have distributed access since Tokio tasks can't be scoped
        let bin_map_arc = Arc::new(bin_map);
        let task_id_gen = task_id.clone();
        let token = token.clone();

        set.spawn(async move {
            ListenerConfig {
                browser,
                listener,
                bin_map_arc,
                task_id_gen,
                token,
            }
            .spawn_listener()
            .await
        });
    }

    // Graceful shutdown helper task
    set.spawn(async move {
        signal::ctrl_c()
            .await
            .map_err(|e| Error::from(e).context("Failed to wait for SIGINT"))
    });

    // Handle responses from tasks
    let mut aborted = false;
    while let Some(result) = set.join_next().await {
        match result {
            Ok(Ok(_)) => (),
            Ok(Err(e)) => Err(e).context("listener task error")?,
            Err(e) => Err(e).context("listener task join failed")?,
        }

        if !aborted {
            aborted = true;
            token.cancel(); // Begin graceful shutdown
        }
    }

    info!("graceful shutdown");
    Ok(())
}
