// (c) Dennis Marttinen 2023
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{anyhow, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub trait OsStringIntoString {
    fn into_string_result(self) -> Result<String>;
}

impl OsStringIntoString for OsString {
    fn into_string_result(self) -> Result<String> {
        self.into_string()
            .map_err(|s| anyhow!("{:?}", s).context("Failed to parse OS string as String"))
    }
}

pub trait PathToStringRef {
    fn to_string_result(&self) -> Result<String>;
}

impl PathToStringRef for Path {
    fn to_string_result(&self) -> Result<String> {
        self.as_os_str().to_os_string().into_string_result()
    }
}

pub trait PathToStringOwned {
    fn into_string_result(self) -> Result<String>;
}

impl PathToStringOwned for PathBuf {
    fn into_string_result(self) -> Result<String> {
        self.into_os_string().into_string_result()
    }
}
