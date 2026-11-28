---
title: Privilege separation
description: How imulup runs pull request code as a separate, unprivileged user on Linux.
---

With privilege separation, PR code runs as a different OS user from imulup. Steps then can't read imulup's GitHub
key, tokens or state, inspect its process, or signal it. It's on by default in the Docker images, and works on Linux
only.

## Setting it up

Two settings turn it on:

| Flag | Environment variable | Docker image default |
| --- | --- | --- |
| `--server-user` | `IMULUP_SERVER_USER` | `imulup` |
| `--step-user` | `IMULUP_STEP_USER` | `imulup-runner` |

imulup must start as root. Outside Docker, create the users first:

```sh
useradd --system --no-create-home --shell /usr/sbin/nologin imulup
useradd --create-home --shell /bin/sh imulup-runner
chown root: app.pem && chmod 400 app.pem
IMULUP_SERVER_USER=imulup IMULUP_STEP_USER=imulup-runner imulup …
```

The step user needs a home directory it can write to. Pulumi keeps its plugins in `~/.pulumi`, and package managers
keep their caches there too.

## What happens at startup

1. **imulup reads its secrets as root.** It reads the app key, so the key file can stay readable by root alone. It
   also looks up both users.
2. **It prepares the data directory:**

   | Path | Owner | Mode | Why |
   | --- | --- | --- | --- |
   | `<data-dir>` | server | `0755` | Listable, because tools like Pulumi list every parent directory |
   | `state.json` | server | `0600` | Locks and plans: server only |
   | `mirrors/` | server | `0755` | The step user can read the mirrors but not write them |
   | `repos/` | step | `0755` | PR checkouts belong to the step user |

   Files left by an earlier version, or by a different user, are re-owned.
3. **It drops privileges permanently.** imulup switches to the server user, and keeps only `CAP_SETUID`, `CAP_SETGID`
   and `CAP_KILL`. Those let it start steps as the step user and stop them when they time out. It clears the
   inheritable and ambient capability sets, sets `no_new_privs`, and checks that the switch worked before it starts
   any threads.

## How a checkout works

1. The server user fetches the PR head into its own bare mirror under `mirrors/`, using credentials.
2. The step user clones or fetches from that mirror over the local filesystem, without credentials, into
   `repos/<owner>/<repo>/<pr>`.
3. Steps run there as the step user, with no capabilities, and with `HOME`, `USER` and `LOGNAME` set to the step
   user's.

git credentials never reach a process the step user owns.

## What steps can and can't do

Steps run as the step user and **can't**:

- read the app key file, `state.json`, or imulup's environment or memory
- write to the mirrors
- signal or trace imulup
- gain privileges through setuid binaries, because `no_new_privs` is set

They **can**:

- read the mirrors, which is how checkouts work
- read and write every PR's checkout, and the step user's home directory and caches, because all PRs share one user
- use every credential in imulup's environment other than `IMULUP_*` variables, such as `PULUMI_ACCESS_TOKEN` and
  cloud credentials

The [security model](/imulup/security/model/#what-imulup-cant-protect) explains what that means in practice.

## Turning it off

Set both variables to empty values (`IMULUP_SERVER_USER=` and `IMULUP_STEP_USER=`) to run everything as the starting
user. imulup warns when it runs as root without `--server-user`, and when steps share its user. Only do this for
local testing.

## Running on other platforms

On macOS and other non-Linux systems, `--server-user` fails at startup. Leave both settings unset there. imulup then
runs steps as its own user.
