#!/bin/sh -e

if [ "$#" -eq 0 ]; then
	cat <<-EOF
		Usage: $0 <browser>...
		<browser> refers to the name of a browser entry in the configuration file of
		the nm-proxy daemon. Examples include "firefox", "librewolf", and "chromium".
	EOF

	exit 1
fi

rustup target add x86_64-unknown-linux-musl
cargo build --release

DAEMON_PATH=$(readlink -f target/x86_64-unknown-linux-musl/release/daemon)
SETUP_PATH=$(readlink -f target/x86_64-unknown-linux-musl/release/setup)

for browser in "$@"; do
	systemctl --user stop "nm-proxy@$browser.socket" || true
done

systemctl --user stop nm-proxy.service || true

cat >~/.config/systemd/user/nm-proxy-setup.service <<EOF
[Unit]
Description=nm-proxy setup helper

[Service]
#Environment=RUST_LOG=trace
ExecStart="$SETUP_PATH"

[Install]
WantedBy=default.target
EOF

cat >~/.config/systemd/user/nm-proxy.service <<EOF
[Unit]
Description=nm-proxy daemon

[Service]
#Environment=RUST_LOG=trace
ExecStart="$DAEMON_PATH"
KillSignal=SIGINT
NonBlocking=true
EOF

# The ListenStreams can't be touched after the socket services have been started without losing the FDs
cat >~/.config/systemd/user/nm-proxy@.socket <<EOF
[Unit]
Description=nm-proxy daemon sockets

[Socket]
Service=nm-proxy.service
ListenStream=%t/nm-proxy-%I.socket
FileDescriptorName=%I

[Install]
WantedBy=sockets.target
EOF

systemctl --user daemon-reload

systemctl --user enable nm-proxy-setup.service
systemctl --user start nm-proxy-setup.service

for browser in "$@"; do
	systemctl --user enable "nm-proxy@$browser.socket"
	systemctl --user start "nm-proxy@$browser.socket"
done
