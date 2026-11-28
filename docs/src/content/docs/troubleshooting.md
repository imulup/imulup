---
title: Troubleshooting
description: Fixes for common problems with imulup's startup, webhooks, plans and locks.
---

Start with the logs (`docker compose logs imulup`, or `journalctl -u imulup`), and with **Recent Deliveries** in the
GitHub App or webhook settings. For more detail, set `RUST_LOG=imulup=debug`: imulup then logs why it ignored each
webhook.

## imulup won't start

**`the following required arguments were not provided: <--gh-app-key <GH_APP_KEY>|--gh-app-key-file <GH_APP_KEY_FILE>>`**

`IMULUP_GH_APP_ID` is set, but no key is. With Compose, the `secrets:` block only mounts the file. Also set
`IMULUP_GH_APP_KEY_FILE=/run/secrets/gh_app_key`.

**`the argument '--gh-app-id <GH_APP_ID>' cannot be used with '--gh-token <GH_TOKEN>'`**

Both auth methods are set. An empty variable counts as set, so comment out `IMULUP_GH_TOKEN=` rather than
leaving it blank.

**`unsafe --gh-app-key-file: … has permissions 644, so other users can read it`**

Run `chmod 600` on the key file. In Kubernetes, set the secret volume's `defaultMode` to `0400`.

**`GitHub App private key is not a valid RSA PEM key`**

Pass the `.pem` file GitHub generated, unmodified. If you use `IMULUP_GH_APP_KEY`, check that the newlines
survived.

**`authenticating as the GitHub App`**, followed by `401`

The app ID doesn't match the key, or the key was deleted in the app settings.

**`--server-user is only supported on Linux`**

Leave `IMULUP_SERVER_USER` and `IMULUP_STEP_USER` unset outside Linux.

**`no such user "imulup-runner"`**

Create the users first. See [Privilege separation](/security/privilege-separation/#setting-it-up).

## Nothing happens on a pull request

Check the delivery in **Recent Deliveries**:

| Response | Fix |
| --- | --- |
| No delivery at all | The app or webhook isn't subscribed to *Pull request* and *Issue comment*, or the app isn't installed on the repository. |
| Failed to connect, or `502` | imulup isn't running or isn't reachable. Check the container and your proxy. With nginx, `connect() failed (111: Connection refused)` means nothing is listening on the upstream port. |
| `401 bad signature` | The webhook secret differs from `IMULUP_GH_WEBHOOK_SECRET`. |
| `403 repo not allowlisted` | Add the repository to `IMULUP_REPO_ALLOWLIST`. |
| `400 invalid JSON` | Set the webhook content type to `application/json`. |
| `202 accepted`, but no comment | See the imulup logs for the error. |

If the delivery was accepted but nothing appears:

- **No Pulumi project was modified.** imulup logs `no Pulumi projects modified`, and doesn't comment. Check
`when_modified`, or the location of `Pulumi.yaml` and the `Pulumi.<stack>.yaml` files.
- **The PR is a draft or from a fork.** See `--plan-drafts` and `--allow-fork-prs`.
- **Your comments are ignored.** With a token, imulup ignores comments from `IMULUP_GH_USER`. If that's your own
account, use a separate bot account.
- **`the GitHub App is not installed on <repo>`.** Install the app on that repository.
- **`creating an installation token for <repo> returned 422`.** The app lacks a permission imulup asks for. Check the
[permissions](/github/app/#repository-permissions). After changing them, accept the new permissions on each
installation.

## Plans fail

**`error: no stack named 'dev' found`**

The stack doesn't exist in the backend. Create it with `pulumi stack init`, or add
`run: pulumi stack select --create "$IMULUP_STACK"` to the plan workflow. See
[Stack selection](/pulumi/stacks/#stacks-must-exist).

**`NoSuchBucket`**, or other errors from the state backend

Check `PULUMI_BACKEND_URL`. The bucket must exist, the region must match, and S3-compatible storage needs an
`endpoint`. See [State backends](/pulumi/backends/).

**`PULUMI_ACCESS_TOKEN must be set for login during non-interactive CLI sessions`**

Set `PULUMI_ACCESS_TOKEN`, or set `PULUMI_BACKEND_URL` for a self-managed backend.

**`passphrase must be set with PULUMI_CONFIG_PASSPHRASE or PULUMI_CONFIG_PASSPHRASE_FILE`**

The stack uses the passphrase secrets provider. Set one of the two.

**`Permission denied` from `pip install`, `npm install -g` or `apt-get`**

Steps run as an unprivileged user. Install into the project instead: a virtualenv for Python (see
[Workflows](/usage/workflows/#python-virtualenvs)), or a local `node_modules`. Bake system packages into a
[custom image](/install/docker/#building-the-images).

**`detecting policy pack path: open /data: permission denied`**

Fixed in 0.4.0. Upgrade, or run `chmod 755 /data` in the container as a stopgap (it's reset on restart).

**`step timed out after 3600s`**

Raise `IMULUP_STEP_TIMEOUT_SECS`, or find out what's slow. A step waiting for input hangs until it times out,
because stdin is empty.

**`invalid imulup.yaml`**

The error names the problem, such as an unknown key or an undefined workflow. See the
[`imulup.yaml` reference](/reference/repo-config/#validation).

## Applies are skipped

The comment explains each skipped project:

| Message | Fix |
| --- | --- |
| `no plan yet; run imulup plan …` | Plan first. |
| `plan is for commit X but the PR is at Y` | Someone pushed after the plan. Plan again. |
| `the last plan failed` | Fix the failure, and plan again. |
| `already applied at this commit` | Plan again to apply again. |
| `… is locked by #N` | Another PR holds the [lock](/usage/locking/). Merge or close it, or comment `imulup unlock` on it. |
| `apply requirement 'approved' is not met` | Get an approving review from someone other than the author. |
| `apply requirement 'mergeable' is not met` | Resolve conflicts with the base branch. Right after a push, wait a few seconds for GitHub to compute mergeability. |
| `nothing to apply; run imulup plan first` | There's no unapplied, successful plan on the PR. |

## A lock is stuck

If imulup was down when a PR was closed, it missed the `closed` webhook and still holds the PR's locks. `imulup unlock`
on a closed PR is refused. Instead:

1. In **Recent Deliveries**, find the `pull_request` delivery with action `closed`, and choose **Redeliver**.
2. If that delivery is gone, reopen the PR, comment `imulup unlock`, and close it again.

As a last resort, stop imulup, remove the lock from `<data-dir>/state.json`, and start it again.
