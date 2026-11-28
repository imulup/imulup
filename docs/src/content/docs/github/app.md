---
title: GitHub App
description: Connect imulup to GitHub as a GitHub App, the recommended way to authenticate.
---

A GitHub App is the recommended way to connect imulup to GitHub. Compared with a personal access token:

- imulup gets a short-lived installation token for each repository, limited to that repository and to the
  permissions each task needs.
- Comments come from `<app-slug>[bot]`, so you don't need a separate bot user.
- The app's own webhook delivers events, so you don't configure webhooks on each repository.

## Create the app

Create the app under your organization (**Organization settings → Developer settings → GitHub Apps → New GitHub
App**) or your user account (**Settings → Developer settings → GitHub Apps**).

| Setting | Value |
| --- | --- |
| **Homepage URL** | Anything, for example your imulup host |
| **Webhook → Active** | Checked |
| **Webhook URL** | `https://<your-imulup-host>/events` |
| **Webhook secret** | A random value, for example from `openssl rand -hex 32`. Use the same value for `IMULUP_GH_WEBHOOK_SECRET`. |
| **SSL verification** | Enabled |
| **Where can this GitHub App be installed?** | *Only on this account* |

You can leave the OAuth, callback, setup URL and device-flow settings empty. imulup doesn't use them.

### Repository permissions

| Permission | Access | Used for |
| --- | --- | --- |
| Contents | *Read-only* (*Read and write* for [automerge](/imulup/usage/automerge/)) | Fetching PR heads; merging |
| Pull requests | *Read and write* | Reading PRs, changed files and reviews; merging |
| Issues | *Read and write* | PR comments and 👀 reactions (PR comments use the Issues API) |
| Commit statuses | *Read and write* | `imulup/plan` and `imulup/apply` statuses |
| Metadata | *Read-only* | Required by GitHub for every app |

imulup doesn't need any organization or account permissions.

### Events

Subscribe to:

- **Pull request**: opened, reopened, pushed to (`synchronize`), ready for review, and closed
- **Issue comment**: new comments, which may be commands

## Install it and configure imulup

1. Under **Private keys**, generate a key. GitHub downloads a `.pem` file.
2. Choose **Install App** and install it on the repositories imulup should manage. Prefer **Only select
   repositories**.
3. Set the app ID and key on imulup:

   ```sh
   IMULUP_GH_APP_ID=123456
   IMULUP_GH_APP_KEY_FILE=/run/secrets/gh_app_key
   IMULUP_GH_WEBHOOK_SECRET=...
   IMULUP_REPO_ALLOWLIST=my-org/infra,my-org/platform-*
   ```

At startup imulup calls `GET /app` to check the key and logs `authenticated as GitHub App <slug>[bot]`. It ignores
comments from that login, so it never responds to itself.

### The private key

- Pass it as a file with `IMULUP_GH_APP_KEY_FILE`. imulup refuses to start if the file is readable by group or
  others, so `chmod 600` it, or `chmod 400` and root-owned with privilege separation.
- `IMULUP_GH_APP_KEY` with the PEM contents also works, but imulup warns: environment variables leak more easily.
- imulup reads the key once at startup, before it drops privileges. With
  [privilege separation](/imulup/security/privilege-separation/), the file can be readable by root alone.

A leaked app key can mint tokens for every installation of the app. Install the app only where imulup needs it, and
rotate the key if you suspect it leaked: generate a new key, restart imulup with it, then delete the old key in
the app settings.

## How imulup uses installation tokens

For each repository, imulup mints separate installation tokens for separate jobs:

| Token | Permissions | Used for |
| --- | --- | --- |
| API | pull requests, issues, statuses: write | Comments, reactions, statuses, reading PRs |
| git | contents: read | Fetching the PR head into imulup's mirror |
| merge | contents, pull requests: write | Automerge only, minted at merge time |

Each token covers only the one repository. Tokens are refreshed after 45 minutes, and revoked as soon as no job is
running on that repository.

The repo allowlist still applies. Installing the app on a repository that isn't on `IMULUP_REPO_ALLOWLIST` doesn't
let imulup act on it: imulup answers its webhooks with `403 repo not allowlisted`.

## Testing the webhook

In the app settings, **Advanced → Recent Deliveries** shows every webhook with its response:

| Response | Meaning |
| --- | --- |
| `202 accepted` | imulup is handling the event |
| `200 pong` | the `ping` event GitHub sends when the app is created |
| `200` with a reason | imulup ignored the event, for example `pull_request action not handled` |
| `401 bad signature` | the webhook secret doesn't match `IMULUP_GH_WEBHOOK_SECRET` |
| `403 repo not allowlisted` | the repository isn't on `IMULUP_REPO_ALLOWLIST` |

Use **Redeliver** to resend one after fixing the configuration.
