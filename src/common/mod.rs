// (c) Dennis Marttinen 2023
// SPDX-License-Identifier: GPL-3.0-or-later

use std::env;
use std::env::VarError;
use std::io::Error as IoError;
use std::io::{ErrorKind, IoSlice};

use anyhow::{Context, Result};
use byteorder::ByteOrder;
use byteorder::NativeEndian;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const SOCKET_PREFIX: &str = "nm-proxy-";
pub const EXTENSION_KEY: &str = "extension";
pub const CONFIG_DIR: &str = "nm-proxy";
pub const CONFIG_FILE: &str = "config.toml";
pub const APP_MANIFEST_DIR: &str = "manifest";
pub const PROXY_CLIENT_BIN: &str = "nm-proxy-client";

pub mod traits;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)] // Strict mode
pub struct HandshakeMessage {
    pub manifest_name: String,
    pub args: Vec<String>,
}

pub fn parse_env(name: &str, default: Option<&str>) -> Result<String> {
    let result = env::var(name);
    if let (Err(VarError::NotPresent), Some(value)) = (&result, default) {
        return Ok(value.into());
    }

    result.with_context(|| format!("Failed to parse environment variable {}", name))
}

pub async fn send_nm_object(
    writer: &mut (impl AsyncWrite + Unpin),
    object: impl Serialize,
) -> Result<()> {
    let data = serde_json::to_vec(&object).context("Serializing object failed")?;

    let mut len_buf = vec![0u8; std::mem::size_of::<u32>()];
    NativeEndian::write_u32(
        len_buf.as_mut_slice(),
        data.len()
            .try_into()
            .context("Attempted to send message message larger than 4 GiB")?,
    );

    writer
        .write_vectored(&[IoSlice::new(&len_buf), IoSlice::new(&data)])
        .await
        .context("Failed to write message")?;

    Ok(())
}

pub async fn recv_nm_object<T: DeserializeOwned>(
    reader: &mut (impl AsyncRead + Unpin),
) -> Result<T> {
    let mut len_buf = vec![0; std::mem::size_of::<u32>()];
    reader
        .read_exact(&mut len_buf)
        .await
        .context("Failed to read message length")?;

    let length: usize = NativeEndian::read_u32(&len_buf)
        .try_into()
        .map_err(|err| IoError::new(ErrorKind::InvalidData, err))
        .context("Failed to parse message length")?;

    let mut buffer = vec![0; length];
    reader
        .read_exact(&mut buffer)
        .await
        .context("Failed to read message")?;

    Ok(serde_json::from_slice(&buffer)?)
}
