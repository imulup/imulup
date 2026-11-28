---
title: Server configuration
description: Every imulup server flag and environment variable.
---

Every flag can also be set with its environment variable. Prefer environment variables for secrets, because command
lines show up in `ps`. `imulup --help` prints the same list.

Boolean variables accept `true`/`false`, `yes`/`no`, `on`/`off` and `1`/`0`.

## GitHub

Set exactly one of `--gh-token` or `--gh-app-id`.

| Flag | Environment variable | Default | Description |
| --- | --- | --- | --- |
| `--gh-app-id` | `IMULUP_GH_APP_ID` | | GitHub App ID. Needs `--gh-app-key-file` or `--gh-app-key`. |
| `--gh-app-key-file` | `IMULUP_GH_APP_KEY_FILE` | | Path to the app's private key (PEM). Read once at startup. Must not be readable by group or others. |
| `--gh-app-key` | `IMULUP_GH_APP_KEY` | | The private key's PEM contents. Prefer the file. |
| `--gh-token` | `IMULUP_GH_TOKEN` | | Personal access token. Needs `--gh-user`. |
| `--gh-user` | `IMULUP_GH_USER` | `<app-slug>[bot]` with an app | The login imulup comments as. Its comments are ignored. Required with a token. |
| `--gh-webhook-secret` | `IMULUP_GH_WEBHOOK_SECRET` | *required* | The webhook secret, used to verify `X-Hub-Signature-256`. |
| `--gh-api-url` | `IMULUP_GH_API_URL` | `https://api.github.com` | API base URL. For GitHub Enterprise Server: `https://<host>/api/v3`. |
| `--repo-allowlist` | `IMULUP_REPO_ALLOWLIST` | *required* | Comma-separated `owner/repo` globs, such as `my-org/*,other/infra`. Matching is case-insensitive, and `*` doesn't match `/`. |

## Behavior

| Flag | Environment variable | Default | Description |
| --- | --- | --- | --- |
| `--disable-autoplan` | `IMULUP_DISABLE_AUTOPLAN` | `false` | Don't plan automatically when PRs are opened or pushed to. |
| `--plan-drafts` | `IMULUP_PLAN_DRAFTS` | `false` | Autoplan draft PRs too. |
| `--allow-fork-prs` | `IMULUP_ALLOW_FORK_PRS` | `false` | Run on PRs from forks. Their code runs with your credentials: see the [security model](/imulup/security/model/). |
| `--skip-collaborator-check` | `IMULUP_SKIP_COLLABORATOR_CHECK` | `false` | Let anyone who can comment run commands, not just users with write access. |
| `--require-approval` | `IMULUP_REQUIRE_APPROVAL` | `false` | Add the `approved` [apply requirement](/imulup/usage/apply-requirements/) to every project. |
| `--require-mergeable` | `IMULUP_REQUIRE_MERGEABLE` | `false` | Add the `mergeable` apply requirement to every project. |
| `--automerge` | `IMULUP_AUTOMERGE` | `false` | Merge PRs once every planned project is applied. Repos can override it in `imulup.yaml`. See [Automerge](/imulup/usage/automerge/). |
| `--automerge-method` | `IMULUP_AUTOMERGE_METHOD` | `merge` | `merge`, `squash` or `rebase`. |
| `--command-name` | `IMULUP_COMMAND_NAME` | `imulup` | The word that starts PR comment commands. |
| `--step-timeout-secs` | `IMULUP_STEP_TIMEOUT_SECS` | `3600` | The longest a single workflow step may run, in seconds. |

## Runtime

| Flag | Environment variable | Default | Description |
| --- | --- | --- | --- |
| `--bind` | `IMULUP_BIND` | `0.0.0.0` | Address to listen on. |
| `--port` | `IMULUP_PORT` | `4141` | Port to listen on. |
| `--data-dir` | `IMULUP_DATA_DIR` | `./imulup-data` (`/data` in the images) | Lock state, git mirrors and PR checkouts. |
| `--pulumi-bin` | `IMULUP_PULUMI_BIN` | `pulumi` | The Pulumi CLI to run. |
| `--server-user` | `IMULUP_SERVER_USER` | unset (`imulup` in the images) | Switch to this user after startup. Needs root. Linux only. Empty disables it. |
| `--step-user` | `IMULUP_STEP_USER` | unset (`imulup-runner` in the images) | Run checkouts and steps as this user. Empty disables it. See [Privilege separation](/imulup/security/privilege-separation/). |

## Other environment variables

| Variable | Description |
| --- | --- |
| `RUST_LOG` | Log filter, such as `info` (the default), `debug` or `imulup=debug`. |
| `PULUMI_*`, cloud credentials | Not read by imulup. They pass through to steps. See [Cloud credentials](/imulup/pulumi/credentials/). |

imulup sets `PULUMI_SKIP_UPDATE_CHECK=true` and `CI=true` for every step.

:::caution
An empty environment variable still counts as set. `IMULUP_GH_TOKEN=` next to `IMULUP_GH_APP_ID=123` is two auth
methods, and imulup refuses to start. Comment out variables you don't use.
:::
