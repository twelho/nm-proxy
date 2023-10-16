# nm-proxy

[Native messaging](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/Native_messaging) proxy for Flatpak'ed browsers. Temporary solution until https://github.com/flatpak/xdg-desktop-portal/issues/655 gets resolved, potentially by https://github.com/flatpak/xdg-desktop-portal/pull/705.

## Architecture

`nm-proxy` consists of a client and daemon binary. The client binary is executed by the Flatpak'ed browser, and forwards stdio through an exposed socket to the daemon on the host, which runs the native binary and forwards the socket traffic to its stdio.

The daemon is intended to be run as a systemd user service, and will read its configuration as well as the [native manifests](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/Native_manifests) from `~/.config/nm-proxy` (configuration guidelines will be printed if missing). On launch, the daemon takes care of installing the client binary and manifest as well configuring the Flatpak environment for each specified browser.

## Building

```shell
cargo build --bin client --release
cargo build --bin daemon --release
```

## Acknowledgements

`nm-proxy` is heavily inspired by the following projects:

- [keepassxc-proxy-rust](https://github.com/varjolintu/keepassxc-proxy-rust/commit/0b812153a8e8d7bc9783fb53b4f388eba4bf0e9d) by Sami Vänttinen ([@varjolintu](https://github.com/varjolintu))
- [native-messaging-proxy](https://github.com/leenr/native-messaging-proxy/commit/9a98f1913b7714efdc0f8b23a0678bc437a89811) by Vladimir Solomatin ([@leenr](https://github.com/leenr))

## Authors

- Dennis Marttinen ([@twelho](https://github.com/twelho))

## License

[GPL-3.0-or-later](https://spdx.org/licenses/GPL-3.0-or-later.html) ([LICENSE](LICENSE))
