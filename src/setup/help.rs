// (c) Dennis Marttinen 2023
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use std::path::Path;

pub trait ManifestHelpContext {
    fn manifest_help_context(self, manifest_dir: impl AsRef<Path>) -> Self;
}

impl<T> ManifestHelpContext for Result<T> {
    fn manifest_help_context(self, manifest_dir: impl AsRef<Path>) -> Self {
        self.with_context(|| format!(
            r#"
No manifests found. In order to use nm-proxy, please place (unmodified) app manifests [1] in the manifest directory at
{},
optionally in a sub-directory named after a browser configuration (e.g., "firefox") to scope it to a particular browser.

[1]: https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/Native_messaging#app_manifest"#,
            manifest_dir.as_ref().display())
        )
    }
}
