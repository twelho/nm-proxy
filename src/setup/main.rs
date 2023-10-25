// (c) Dennis Marttinen 2023
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{bail, Context, Result};
use ini::Error::Io;
use ini::Ini;
use serde_json::Value;
use std::io::ErrorKind;
use std::path::Path;
use tokio::fs;
use tokio::fs::DirEntry;
use tokio::io::AsyncReadExt;
use tracing::{debug, info, instrument};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::EnvFilter;

use nm_proxy::common;
use nm_proxy::common::config;
use nm_proxy::common::config::Config;
use nm_proxy::common::constants::*;
use nm_proxy::common::runtime::{NativeBinaryMap, Settings};
use nm_proxy::common::traits::*;

mod help;

use help::ManifestHelpContext;

#[instrument(skip(nmh_dir), fields(browser = _browser, nmh_dir = %nmh_dir.as_ref().display()))]
async fn create_nmh_dir(_browser: &str, nmh_dir: impl AsRef<Path>) -> Result<()> {
    let nmh_dir = nmh_dir.as_ref();

    match fs::create_dir(nmh_dir).await {
        Ok(_) => (),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => (),
        result @ Err(_) => result
            .with_context(|| format!("{}", nmh_dir.display()))
            .context("Unable to create NMH directory, is the browser configuration correct?")?,
    }

    Ok(())
}

#[instrument(skip(nmh_dir, config), fields(nmh_dir = %nmh_dir.as_ref().display()))]
async fn install_proxy_client(
    browser: &str,
    nmh_dir: impl AsRef<Path>,
    config: &Config,
) -> Result<()> {
    let nmh_dir = nmh_dir.as_ref();
    let proxy_client_src = config.proxy_client_path();
    let proxy_client_dest = nmh_dir.join(PROXY_CLIENT_BIN);

    fs::copy(&proxy_client_src, &proxy_client_dest)
        .await
        .with_context(|| {
            format!(
                "{} -> {}",
                proxy_client_src.display(),
                proxy_client_dest.display()
            )
        })
        .with_context(|| format!("{}: unable to copy proxy client", browser))?;

    Ok(())
}

#[instrument(skip(path), fields(path = %path.as_ref().display()))]
async fn configure_flatpak_overrides(browser: &str, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    let mut ini = match Ini::load_from_file(&path) {
        Ok(i) => i,
        Err(Io(e)) if e.kind() == ErrorKind::NotFound => Ini::new(),
        result @ Err(_) => result
            .with_context(|| path.display().to_string())
            .with_context(|| format!("Unable to read Flatpak overrides for {browser}"))?,
    };

    set_socket_path_override(browser, &mut ini);
    ini.write_to_file(&path)
        .with_context(|| path.display().to_string())
        .with_context(|| format!("Unable to update Flatpak overrides for {browser}"))?;

    Ok(())
}

#[instrument(level = "trace", skip(path), fields(path = %path.as_ref().display()))]
async fn read_manifest(path: impl AsRef<Path>) -> Result<Value> {
    let mut contents = String::new();
    fs::File::open(path)
        .await?
        .read_to_string(&mut contents)
        .await?;

    Ok(serde_json::from_str(&contents)?)
}

#[instrument(skip_all, fields(browser = _browser, path = %entry.path().display()))]
async fn install_manifest(entry: &DirEntry, _browser: &str, nmh_dir: &Path) -> Result<String> {
    // Read the manifest
    let path = entry.path();
    let mut manifest = read_manifest(&path)
        .await
        .with_context(|| path.display().to_string())
        .context("Unable to read app manifest")?;

    // Extract the "path" field
    let path = match &manifest["path"] {
        Value::String(s) => s.into(),
        _ => bail!("Malformed app manifest, \"path\" key missing"),
    };

    // Check that the "type" field is "stdio" (other formats are currently unsupported)
    match &manifest["type"] {
        Value::String(s) if s == "stdio" => (),
        _ => bail!("Unsupported app manifest, only type \"stdio\" is currently supported"),
    }

    // Replace the path with the proxy client path
    manifest["path"] = nmh_dir.join(PROXY_CLIENT_BIN).into_string_result()?.into();

    // Write the modified app manifest into the NMH directory
    let deployment_path = nmh_dir.join(entry.file_name());
    fs::write(&deployment_path, serde_json::to_vec_pretty(&manifest)?)
        .await
        .with_context(|| deployment_path.display().to_string())
        .context("Failed to deploy app manifest")?;
    Ok(path)
}

#[instrument(level = "trace", skip_all)]
async fn install_manifests(config: &Config, path: impl AsRef<Path>) -> Result<NativeBinaryMap> {
    let manifest_dir = path.as_ref().join(APP_MANIFEST_DIR);

    // Open the manifest directory as a stream
    let mut stream = match fs::read_dir(&manifest_dir).await {
        Ok(s) => s,
        result @ Err(_) => {
            let kind = result.as_ref().err().map(|e| e.kind());
            let result = result.path_context(&manifest_dir);
            match kind {
                Some(ErrorKind::NotFound) => result.manifest_help_context(&manifest_dir),
                _ => result,
            }?
        }
    };

    let mut native_binary_map = NativeBinaryMap::new();

    // Install common manifests
    while let Some(entry) = stream.next_entry().await.path_context(&manifest_dir)? {
        let metadata = entry.metadata().await.path_context(entry.path())?;
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
                result @ Err(_) => result.path_context(&browser_manifest).map(|_| ())?,
            }

            // Install the manifest
            let nmh_path = install_manifest(&entry, browser, &nmh_dir).await?;

            // Track native binary paths per browser for host-side execution
            native_binary_map
                .entry(browser.into())
                .or_insert(Default::default())
                .insert(file_name.clone(), nmh_path);
        }
    }

    // Install browser-specific manifests
    for (browser, nmh_dir) in config.nmh_dirs()? {
        let br_manifest_dir = manifest_dir.join(browser);
        let mut stream = match fs::read_dir(&br_manifest_dir).await {
            Ok(s) => s,
            Err(e) if e.kind() == ErrorKind::NotFound => continue, // Skip if not found
            result @ Err(_) => result.path_context(&br_manifest_dir)?,
        };

        while let Some(entry) = stream.next_entry().await.path_context(&br_manifest_dir)? {
            let metadata = entry.metadata().await.path_context(entry.path())?;
            let file_name = entry.file_name().into_string_result()?;
            if !metadata.is_file() || !file_name.ends_with(".json") {
                continue; // Skip all entries that are not app manifests
            }

            // Install the manifest
            let nmh_path = install_manifest(&entry, browser, &nmh_dir).await?;

            // Track native binary paths per browser for host-side execution
            native_binary_map
                .entry(browser.into())
                .or_insert(Default::default())
                .insert(file_name, nmh_path);
        }
    }

    Ok(native_binary_map)
}

#[instrument(level = "trace", skip(config))]
fn set_socket_path_override(browser: &str, config: &mut Ini) {
    let filesystems = config
        .section(Some("Context"))
        .map(|s| s.get("filesystems"))
        .flatten()
        .unwrap_or("")
        .to_owned();

    let socket_path = format!("xdg-run/{SOCKET_PREFIX}{browser}{SOCKET_SUFFIX}");
    if filesystems.split(";").any(|e| e == socket_path) {
        return; // Already configured
    }

    config.with_section(Some("Context")).set(
        "filesystems",
        match &*filesystems {
            "" => socket_path,
            s => format!("{s};{socket_path}"),
        },
    );
}

#[tokio::main]
#[instrument]
async fn main() -> Result<()> {
    // Initialize the logging framework
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .with_span_events(FmtSpan::NEW)
        .init();

    // Acquire the runtime directory path
    let runtime_dir = common::parse_env("XDG_RUNTIME_DIR", None)?;

    // Load configuration
    let config_path = config::form_config_path().await?;
    let config = config::load_config(&config_path).await?;
    debug!("configuration: {:?}", config);

    for (browser, nmh_dir) in config.nmh_dirs()? {
        // Create native messaging host directory
        create_nmh_dir(browser, &nmh_dir).await?;

        // Install proxy client
        install_proxy_client(browser, &nmh_dir, &config).await?;
    }

    // Configure Flatpak overrides
    for (browser, path) in config.override_paths()? {
        configure_flatpak_overrides(browser, &path).await?;
    }

    // Install manifests
    let native_binaries = install_manifests(&config, &config_path).await?;
    debug!("native binary map: {:?}", native_binaries);

    // Save runtime configuration
    Settings { native_binaries }.save(runtime_dir).await?;

    info!("setup complete");
    Ok(())
}
