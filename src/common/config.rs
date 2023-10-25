// (c) Dennis Marttinen 2023
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::common;
use crate::common::constants::*;
use anyhow::{Context, Error, Result};
use expanduser::expanduser;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::AsyncReadExt;

const CONFIG_HELP: &str = concat!(
    r#"
nm-proxy needs a configuration file before it can operate.
Ensure that it is present, and contains the following:

# nm-proxy "#,
    env!("CARGO_PKG_VERSION"),
    r#" configuration file
#
# [daemon]
# proxy_client = "/path/to/client" # Path to nm-proxy client binary
#
# [browsers.<name>] # Define configuration for browser <name>
# app_id = "app.example.com" # Flatpak 3-part app ID
# nmh_dir = ".<name>/native-messaging-hosts" # Native messaging host application directory
#
# Example configuration:

[daemon]
proxy_client = "~/path/to/client"

[browsers.firefox]
app_id = "org.mozilla.firefox"
nmh_dir = ".mozilla/native-messaging-hosts"

[browsers.librewolf]
app_id = "io.gitlab.librewolf-community"
nmh_dir = ".librewolf/native-messaging-hosts"

[browsers.chromium]
app_id = "org.chromium.Chromium"
nmh_dir = ".config/chromium/NativeMessagingHosts""#
);

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)] // Strict mode
struct DaemonConfig {
    #[serde(deserialize_with = "path_parser")]
    proxy_client: PathBuf,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)] // Strict mode
struct BrowserConfig {
    app_id: String,
    nmh_dir: String,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)] // Strict mode
pub struct Config {
    daemon: DaemonConfig,
    browsers: HashMap<String, BrowserConfig>,
}

impl Config {
    pub fn browsers(&self) -> impl Iterator<Item = &String> {
        self.browsers.keys()
    }

    pub fn nmh_dirs(&self) -> Result<impl Iterator<Item = (&String, PathBuf)> + '_> {
        let app_dir = expanduser("~/.var/app").context("Path expansion failed")?;

        Ok(self.browsers.iter().map(move |(n, c)| {
            let mut d = app_dir.join(&c.app_id);
            d.push(&c.nmh_dir);
            (n, d)
        }))
    }

    pub fn override_paths(&self) -> Result<impl Iterator<Item = (&String, PathBuf)> + '_> {
        let mut config_dir =
            expanduser(common::parse_env("XDG_DATA_HOME", Some("~/.local/share"))?)
                .context("Path expansion failed")?;
        config_dir.push("flatpak");
        config_dir.push("overrides");

        Ok(self
            .browsers
            .iter()
            .map(move |(n, c)| (n, config_dir.join(&c.app_id))))
    }

    pub fn proxy_client_path(&self) -> &PathBuf {
        &self.daemon.proxy_client
    }
}

/// Parse (expand) paths during deserialization
fn path_parser<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
    let s: String = Deserialize::deserialize(deserializer)?;
    expanduser(s).map_err(|e| D::Error::custom(e))
}

pub async fn form_config_path() -> Result<PathBuf> {
    let mut path = expanduser(common::parse_env("XDG_CONFIG_HOME", Some("~/.config"))?)
        .context("Configuration file path expansion failed")?;
    path.push(CONFIG_DIR);
    path.canonicalize()
        .context("Configuration file path canonicalization failed")
}

async fn read_config(config_path: &Path) -> Result<Config> {
    let mut config_file = File::open(&config_path).await?;
    let mut contents = String::new();
    config_file.read_to_string(&mut contents).await?;
    toml::from_str(&contents).map_err(|e| Error::from(e))
}

pub async fn load_config(path: impl AsRef<Path>) -> Result<Config> {
    let path = path.as_ref().join(CONFIG_FILE);
    read_config(&path)
        .await
        .with_context(|| format!("{}", path.display()))
        .context(CONFIG_HELP)
}
