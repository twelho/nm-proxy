// (c) Dennis Marttinen 2023
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use std::fs::{FileType, Metadata};
use std::io::Result as IoResult;
use std::path::Path;
use tokio::fs::{DirEntry, ReadDir};
use tokio::net::UnixListener;

pub trait ErrorContext<T> {
    fn path_context(self, path: impl AsRef<Path>) -> Result<T>;
}

impl ErrorContext<Metadata> for IoResult<Metadata> {
    fn path_context(self, path: impl AsRef<Path>) -> Result<Metadata> {
        self.with_context(|| path.as_ref().display().to_string())
            .context("Unable to read metadata")
    }
}

impl ErrorContext<FileType> for IoResult<FileType> {
    fn path_context(self, path: impl AsRef<Path>) -> Result<FileType> {
        self.with_context(|| path.as_ref().display().to_string())
            .context("Unable to file type")
    }
}

impl ErrorContext<Option<DirEntry>> for IoResult<Option<DirEntry>> {
    fn path_context(self, path: impl AsRef<Path>) -> Result<Option<DirEntry>> {
        self.with_context(|| path.as_ref().display().to_string())
            .context("Unable to access directory entry")
    }
}

impl ErrorContext<ReadDir> for IoResult<ReadDir> {
    fn path_context(self, path: impl AsRef<Path>) -> Result<ReadDir> {
        self.with_context(|| path.as_ref().display().to_string())
            .context("Unable to read directory")
    }
}

impl ErrorContext<UnixListener> for IoResult<UnixListener> {
    fn path_context(self, path: impl AsRef<Path>) -> Result<UnixListener> {
        self.with_context(|| path.as_ref().display().to_string())
            .context("Failed bind to socket")
    }
}
