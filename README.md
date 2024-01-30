# nm-proxy

[![Build](https://github.com/twelho/nm-proxy/actions/workflows/build.yaml/badge.svg)](https://github.com/twelho/nm-proxy/actions/workflows/build.yaml)
[![dependency status](https://deps.rs/repo/github/twelho/nm-proxy/status.svg)](https://deps.rs/repo/github/twelho/nm-proxy)

[Native messaging](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/Native_messaging) proxy for Flatpak'ed browsers. Temporary solution until https://github.com/flatpak/xdg-desktop-portal/issues/655 gets resolved, potentially by https://github.com/flatpak/xdg-desktop-portal/pull/705.

## Architecture

`nm-proxy` consists of a client, daemon, and setup binary. The client binary is executed by the Flatpak'ed browser, and forwards stdio through an exposed socket to the daemon on the host, which runs the native binary and forwards the socket traffic to its stdio. Sockets are handled by systemd to enable transparent daemon restarts without losing the inodes forwarded into the Flatpak namespaces.

The daemon is intended to be run as a systemd user service, and will read its configuration as well as the [native manifests](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/Native_manifests) from `~/.config/nm-proxy` (guidelines will be printed if configuration is missing). The setup binary helps the daemon take care of installing the client binary and manifest as well configuring the Flatpak environment for each specified browser.

The manifests themselves (`.json` files) must be supplied by the user. Here is the upstream manifest of the Plasma Integration extension, with which `nm-proxy` was tested during development:

```json
{
  "name": "org.kde.plasma.browser_integration",
  "description": "Native connector for KDE Plasma",
  "path": "/usr/bin/plasma-browser-integration-host",
  "type": "stdio",
  "allowed_extensions": ["plasma-browser-integration@kde.org"]
}
```

> PSA: If using the Plasma Integration extension, remember to disable native MPRIS support to avoid double media controls:
> 
> - In Firefox-based browsers, disable `Control media via keyboard, headset, or virtual interface` in `about:preferences`
> - In Chromium-based browsers, disable the flag `chrome://flags/#hardware-media-key-handling`

## Configuration

```toml
# nm-proxy 0.2.0 configuration file
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
nmh_dir = ".config/chromium/NativeMessagingHosts"
```

## Installation

```shell
$ ./install.sh
Usage: ./install.sh <browser>...
<browser> refers to the name of a browser entry in the configuration file of
the nm-proxy daemon. Examples include "firefox", "librewolf", and "chromium".
```

## Building

The following builds all three binaries:

```shell
rustup target add x86_64-unknown-linux-musl
cargo build --release
```

## Acknowledgements

`nm-proxy` is heavily inspired by the following projects:

- [keepassxc-proxy-rust](https://github.com/varjolintu/keepassxc-proxy-rust/commit/0b812153a8e8d7bc9783fb53b4f388eba4bf0e9d) by Sami Vänttinen ([@varjolintu](https://github.com/varjolintu))
- [native-messaging-proxy](https://github.com/leenr/native-messaging-proxy/commit/9a98f1913b7714efdc0f8b23a0678bc437a89811) by Vladimir Solomatin ([@leenr](https://github.com/leenr))

## Authors

- Dennis Marttinen ([@twelho](https://github.com/twelho))

## License

[GPL-3.0-or-later](https://spdx.org/licenses/GPL-3.0-or-later.html) ([LICENSE](LICENSE))
