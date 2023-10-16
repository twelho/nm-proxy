// (c) Dennis Marttinen 2023
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::client::ClientTaskConfig;
use crate::config::Config;
use anyhow::{anyhow, bail, Context, Error, Result};
use ini::Error::Io;
use ini::Ini;
use log::{debug, error, info};
use nm_proxy::common;
use nm_proxy::common::traits::*;
use nm_proxy::common::SOCKET_PREFIX;
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::task::JoinSet;
use tokio::{fs, net};

mod client;
mod config;

fn set_socket_path_override(name: &str, config: &mut Ini) {
    let filesystems = config
        .section(Some("Context"))
        .map(|s| s.get("filesystems"))
        .flatten()
        .unwrap_or("")
        .to_owned();

    let socket_path = format!("xdg-run/{SOCKET_PREFIX}{name}");
    if filesystems.split(";").any(|e| e == socket_path) {
        return; // Already configured
    }

    config.with_section(Some("Context")).set(
        "filesystems",
        if filesystems.is_empty() {
            socket_path
        } else {
            format!("{filesystems};{socket_path}")
        },
    );
}

type NativeBinaryMap = HashMap<String, HashMap<String, String>>;

async fn install_manifest(entry: &fs::DirEntry, nmh_dir: &Path) -> Result<String> {
    let file_name = entry.file_name().into_string_result()?;
    let mut app_manifest = fs::File::open(entry.path()).await?;
    let mut contents = String::new();
    app_manifest.read_to_string(&mut contents).await?;
    let mut contents: Value = serde_json::from_str(&contents)?;

    let path = if let Value::String(path) = &contents["path"] {
        path.into()
    } else {
        bail!("Malformed app manifest, \"path\" key missing");
    };

    match &contents["type"] {
        Value::String(s) if s == "stdio" => (),
        _ => bail!("Unsupported app manifest, only type \"stdio\" is currently supported"),
    }

    // Replace the path with the proxy client path
    contents["path"] = nmh_dir
        .join(common::PROXY_CLIENT_BIN)
        .into_string_result()?
        .into();

    // Write the modified app manifest into the NMH directory
    fs::write(
        nmh_dir.join(file_name),
        serde_json::to_vec_pretty(&contents)?,
    )
    .await?;
    Ok(path)
}

async fn read_manifests(config: &Config) -> Result<NativeBinaryMap> {
    let mut manifest_dir = config::form_config_path().await?;
    manifest_dir.push(common::APP_MANIFEST_DIR);

    let mut native_binary_map = NativeBinaryMap::new();

    // TODO: This should output some help text related to
    //  installing manifests if the directory is absent
    let mut stream = fs::read_dir(&manifest_dir)
        .await
        .with_context(|| format!("Failed to read {}", manifest_dir.display()))?;

    while let Some(entry) = stream
        .next_entry()
        .await
        .with_context(|| format!("Failed to access entry in {}", manifest_dir.display()))?
    {
        let metadata = entry.metadata().await?;
        let file_name = entry.file_name().into_string_result()?;
        if !metadata.is_file() || !file_name.ends_with(".json") {
            continue; // Skip all entries that are not app manifests
        }

        for (browser, nmh_dir) in config.nmh_dirs()? {
            let mut browser_manifest = manifest_dir.join(browser);
            browser_manifest.push(&file_name);
            match fs::metadata(&browser_manifest).await {
                // If an equivalent browser-specific manifest exists, it has precedence
                Ok(m) if m.is_file() => continue,
                Ok(m) => bail!("Expected file, found {:?}", m.file_type()),
                Err(e) if e.kind() == ErrorKind::NotFound => (),
                result @ Err(_) => result.map(|_| ()).with_context(|| {
                    format!("Unable to read metadata of {}", browser_manifest.display())
                })?,
            }

            let nmh_path = install_manifest(&entry, &nmh_dir).await?;

            // Track native binary paths per browser for host-side execution
            native_binary_map
                .entry(browser.into())
                .or_insert(Default::default())
                .insert(file_name.clone(), nmh_path);
        }
    }

    for (browser, nmh_dir) in config.nmh_dirs()? {
        let browser_manifest_dir = manifest_dir.join(browser);
        let mut stream = match fs::read_dir(&browser_manifest_dir).await {
            Ok(s) => s,
            Err(e) if e.kind() == ErrorKind::NotFound => continue, // Skip if not found
            result @ Err(_) => {
                result.with_context(|| format!("Failed to read {}", manifest_dir.display()))?
            }
        };

        while let Some(entry) = stream
            .next_entry()
            .await
            .with_context(|| format!("Failed to access entry in {}", manifest_dir.display()))?
        {
            let metadata = entry.metadata().await?;
            let file_name = entry.file_name().into_string_result()?;
            if !metadata.is_file() || !file_name.ends_with(".json") {
                continue; // Skip all entries that are not app manifests
            }

            let nmh_path = install_manifest(&entry, &nmh_dir).await?;

            // Track native binary paths per browser for host-side execution
            native_binary_map
                .entry(browser.into())
                .or_insert(Default::default())
                .insert(file_name, nmh_path);
        }
    }

    Ok(native_binary_map)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging framework
    tracing_subscriber::fmt::init();

    // Parse this early for early error
    let runtime_dir = PathBuf::from(common::parse_env("XDG_RUNTIME_DIR", None)?);

    // Load configuration
    let config = config::load_config().await?;

    for (_, nmh_dir) in config.nmh_dirs()? {
        // Create native messaging host directory
        match fs::create_dir(&nmh_dir).await {
            Ok(_) => (),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => (),
            result @ Err(_) => result
                .with_context(|| format!("{}", nmh_dir.display()))
                .context("Unable to create NMH directory, is the browser configuration correct?")?,
        }

        // Install proxy client
        let proxy_client_src = config.proxy_client_path();
        let proxy_client_dest = nmh_dir.join(common::PROXY_CLIENT_BIN);
        fs::copy(&proxy_client_src, &proxy_client_dest)
            .await
            .with_context(|| format!("{}", proxy_client_src.display()))
            .with_context(|| {
                format!(
                    "Unable to copy proxy client to {}",
                    proxy_client_dest.display()
                )
            })?;
    }

    let native_binary_map = read_manifests(&config).await?;

    // Configure Flatpak overrides
    for (name, path) in config.override_paths()? {
        let mut ini = match Ini::load_from_file(&path) {
            Ok(i) => i,
            Err(Io(e)) if e.kind() == ErrorKind::NotFound => Ini::new(),
            result @ Err(_) => result
                .with_context(|| format!("{}", path.display()))
                .with_context(|| format!("Unable to read Flatpak overrides for {name}"))?,
        };

        set_socket_path_override(name, &mut ini);
        ini.write_to_file(&path)
            .with_context(|| format!("{}", path.display()))
            .with_context(|| format!("Unable to update Flatpak overrides for {name}"))?;
    }

    debug!("configuration: {:?}", config);
    debug!("native binary map: {:?}", native_binary_map);

    let mut set = JoinSet::new();

    for (browser, binary_map) in native_binary_map {
        let socket_path = runtime_dir.join(format!("{SOCKET_PREFIX}{browser}"));
        match fs::remove_file(&socket_path).await {
            Ok(_) => (),
            Err(e) if e.kind() == ErrorKind::NotFound => (),
            result @ Err(_) => {
                result.with_context(|| format!("Failed to remove {}", socket_path.display()))?
            }
        }

        let listener = net::UnixListener::bind(&socket_path)
            .with_context(|| format!("Failed bind socket {}", socket_path.display()))?;

        let binary_map_arc = Arc::new(binary_map);

        set.spawn(async move {
            info!("Listening on {}", socket_path.display());

            // This will abort all nested tasks when dropped
            let mut client_set = JoinSet::new();

            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let bin_map = binary_map_arc.clone();
                        client_set.spawn(async move {
                            ClientTaskConfig { stream, bin_map }.launch().await;
                        });
                    }
                    Err(e) => {
                        error!("Error accepting client: {}", e);
                    }
                }
            }

            // TODO: We can handle the client task output here if needed
        });
    }

    // TODO: Handle responses from tasks here
    while let Some(_) = set.join_next().await {}

    Ok(())
}
