---
title: Personal access token
description: Connect imulup to GitHub with a bot account's token and a repository webhook.
---

If you can't create a GitHub App, imulup can authenticate as a user with a personal access token.

:::caution[Use a dedicated bot account]
imulup ignores comments from the account that owns the token, so it never reacts to its own comments. If you use your
own token, your own `imulup apply` comments are ignored too. Create a separate machine user, such as
`my-org-imulup`, and give it write access to the repositories.
:::

## Create the token

Sign in as the bot account and create a token under **Settings → Developer settings → Personal access tokens**.

- **Fine-grained token:** limit it to the repositories imulup manages, with these repository permissions:
  - Contents: *Read-only* (*Read and write* for [automerge](/usage/automerge/))
  - Pull requests: *Read and write*
  - Issues: *Read and write*
  - Commit statuses: *Read and write*
  - Metadata: *Read-only*
- **Classic token:** the `repo` scope.

Unlike GitHub App tokens, a personal access token is long-lived, and imulup uses the same token for every repository
and every job. Set an expiry and rotate it.

## Configure imulup

```sh
IMULUP_GH_TOKEN=github_pat_...
IMULUP_GH_USER=my-org-imulup          # the login that owns the token
IMULUP_GH_WEBHOOK_SECRET=...
IMULUP_REPO_ALLOWLIST=my-org/infra
```

`IMULUP_GH_USER` is required with a token. Don't also set `IMULUP_GH_APP_ID`: imulup accepts exactly one of the two.

## Add the webhook

On each repository (**Settings → Webhooks → Add webhook**), or once for the whole organization:

| Setting | Value |
| --- | --- |
| **Payload URL** | `https://<your-imulup-host>/events` |
| **Content type** | `application/json` |
| **Secret** | The same value as `IMULUP_GH_WEBHOOK_SECRET` |
| **Events** | *Let me select individual events* → **Pull requests** and **Issue comments** |

The content type has to be `application/json`. With `application/x-www-form-urlencoded`, imulup answers
`400 invalid JSON`.
