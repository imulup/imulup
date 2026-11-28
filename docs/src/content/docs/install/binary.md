---
title: From source
description: Build imulup with Cargo and run it directly on a Linux host.
---

imulup is a single Rust binary. To run it outside Docker, the host needs:

- `git`
- the Pulumi CLI, plus the language runtimes your programs use
- a Rust toolchain, 1.88 or newer, to build it

Privilege separation works on Linux only. On macOS, imulup runs fine for development, but steps run as the same
user as imulup.

## Build

```sh
git clone https://github.com/imulup/imulup.git
cd imulup
cargo build --release --locked
sudo install -m 0755 target/release/imulup /usr/local/bin/imulup
```

## Create the users

Use one user for the server and one for PR code:

```sh
sudo useradd --system --no-create-home --shell /usr/sbin/nologin imulup
sudo useradd --create-home --shell /bin/sh imulup-runner
```

Store the GitHub App key so only root can read it:

```sh
sudo install -d -m 0700 /etc/imulup
sudo install -m 0400 -o root -g root app.private-key.pem /etc/imulup/app.pem
```

## Run with systemd

imulup has to start as root. It reads the key, prepares the data directory, and then switches permanently to
`--server-user`. Don't set `User=` in the unit.

`/etc/imulup/imulup.env`, mode `0600`:

```sh
IMULUP_GH_APP_ID=123456
IMULUP_GH_APP_KEY_FILE=/etc/imulup/app.pem
IMULUP_GH_WEBHOOK_SECRET=...
IMULUP_REPO_ALLOWLIST=my-org/*
IMULUP_DATA_DIR=/var/lib/imulup
IMULUP_SERVER_USER=imulup
IMULUP_STEP_USER=imulup-runner
IMULUP_BIND=127.0.0.1
PULUMI_ACCESS_TOKEN=...
```

`/etc/systemd/system/imulup.service`:

```ini
[Unit]
Description=imulup
After=network-online.target
Wants=network-online.target

[Service]
EnvironmentFile=/etc/imulup/imulup.env
Environment=RUST_LOG=info
ExecStart=/usr/local/bin/imulup
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now imulup
journalctl -u imulup -f
```

Make sure `pulumi` and the language runtimes are on the `PATH` that steps see, or set `IMULUP_PULUMI_BIN` to an
absolute path. Steps get imulup's environment, apart from the `IMULUP_*` variables. They also get the step user's
`HOME`, `USER` and `LOGNAME`.

## Without privilege separation

If you start imulup as a normal user and leave `IMULUP_SERVER_USER` and `IMULUP_STEP_USER` unset, everything runs
as that user. imulup logs a warning, because steps can then read the key file and the state. That's fine on a laptop,
but not for a shared server.

```sh
export IMULUP_GH_APP_ID=123456
export IMULUP_GH_APP_KEY_FILE=$HOME/.config/imulup/app.pem
export IMULUP_GH_WEBHOOK_SECRET=...
export IMULUP_REPO_ALLOWLIST='my-org/*'
imulup
```
