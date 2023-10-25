// (c) Dennis Marttinen 2023
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{anyhow, bail, Result};
use std::env;

pub mod settings;
pub use settings::*;

pub async fn parse_runtime_dir(context: &str) -> Result<String> {
    let mut args = env::args();
    let invocation_path = args
        .next()
        .ok_or(anyhow!("Unable to acquire invocation path"))?;

    if let (Some(dir), None) = (args.next(), args.next()) {
        return Ok(dir);
    }

    bail!("Usage: {} <runtime-dir>\n{}", invocation_path, context);
}
