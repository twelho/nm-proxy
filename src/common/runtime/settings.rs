// (c) Dennis Marttinen 2023
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::common::constants::*;
use anyhow::Result;
use anyhow::{Context, Error};
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;
use tracing::instrument;

pub type NativeBinaryMap = HashMap<String, HashMap<String, String>>;

#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)] // Strict mode
pub struct Settings {
    pub native_binaries: NativeBinaryMap,
}

impl Settings {
    #[instrument(level = "info", skip(dir), fields(dir = %dir.as_ref().display()))]
    pub async fn save(&self, dir: impl AsRef<Path>) -> Result<()> {
        fs::write(
            dir.as_ref().join(SETTINGS_FILE_NAME),
            &toml::to_string_pretty(self).context("Failed to serialize runtime settings")?,
        )
        .await
        .map_err(|e| Error::from(e).context("Failed to write runtime settings"))
    }

    #[instrument(level = "info", skip(dir), fields(dir = %dir.as_ref().display()))]
    pub async fn load(dir: impl AsRef<Path>) -> Result<Self> {
        toml::from_str(
            &fs::read_to_string(dir.as_ref().join(SETTINGS_FILE_NAME))
                .await
                .map_err(|e| Error::from(e).context("Failed to read runtime settings"))?,
        )
        .context("Failed to deserialize runtime settings")
    }
}
