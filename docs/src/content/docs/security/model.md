---
title: Security model
description: What imulup protects, what it can't, and how to deploy it safely when it holds cloud credentials.
---

imulup holds credentials that can change your infrastructure, and it runs code from pull requests. Read this page
before you deploy it with real credentials.

## The core risk: a plan runs code

A Terraform plan evaluates HCL. A Pulumi preview **runs your program**: TypeScript, Python, Go, C# or Java, with full
access to the machine. A `run:` step in `imulup.yaml` is a shell command. So:

:::danger
Anyone who can open a pull request against an allowlisted repository can run code on the imulup host, with every
credential that steps can see. With autoplan, that happens as soon as the PR is opened, before anyone reviews it.
:::

Apply requirements and approvals don't change this. They gate `imulup apply`, but `imulup plan` already ran the PR's
code. Who can open PRs is the real security boundary:

- **Same-repository PRs** need push access to the repository.
- **Fork PRs** are ignored unless you set `--allow-fork-prs`. Leave it off for any repository that holds real
  credentials, and above all for public ones.

## Who can do what

| Actor | Can |
| --- | --- |
| Anyone on the network | Reach `POST /events`, which rejects requests without a valid signature. Reach the lock view and health check, unless you block them. |
| Any GitHub user who can comment | Nothing by default. Commands need write access. |
| Users with write access | Run `plan`, `apply` and `unlock`, subject to apply requirements. |
| PR authors with push access | Run arbitrary code as the step user, with the cloud credentials steps get. |
| The step user (PR code) | Read and change other PRs' checkouts and the shared caches. It can't read imulup's key, tokens, state, environment or memory when [privilege separation](/security/privilege-separation/) is on. |

## Controls

### Webhook validation

- Every request to `/events` must carry an `X-Hub-Signature-256` HMAC of its body made with the webhook secret.
  Requests without one get `401`, before imulup parses them.
- Events from repositories that aren't on `--repo-allowlist` get `403` and are ignored.
- Use a long random secret (`openssl rand -hex 32`), and serve the endpoint over TLS.

GitHub's signature has no timestamp, and imulup doesn't track delivery IDs. Anyone who captured a signed delivery
could replay it. TLS keeps deliveries from being captured. Rotate the secret if one may have leaked.

### Who can run commands

- Commenters need *write*, *maintain* or *admin* permission on the repository. `--skip-collaborator-check` removes this
  check. Don't use it on public repositories.
- Comments from imulup's own account are ignored.
- `apply` only runs against a successful plan of the PR's current head commit. Any push makes the plan stale.
- [Apply requirements](/usage/apply-requirements/) can require an approval from someone other than the author.
  `imulup.yaml` comes from the PR itself, so a PR can remove the requirements set there. Use `--require-approval`
  for a requirement that PRs can't change.

### Secret handling

- **GitHub App tokens** cover one repository and only the permissions each task needs. They're refreshed every 45
  minutes and revoked once work on the repository finishes. The token that can write code is only minted for
  automerge.
- **imulup's own configuration** is removed from steps' environment: every `IMULUP_*` variable, including the GitHub
  token, the app key and the webhook secret.
- **Redaction.** GitHub tokens (current and previous) and the webhook secret are replaced with `***` in anything
  posted to a PR.
- **git credentials** are passed to git through environment-based configuration. They never appear in `.git/config`,
  command lines or remote URLs. With privilege separation, git with credentials only runs as the server user.
- **The app key file** must not be readable by group or others, or imulup refuses to start. imulup reads it once,
  as root, before dropping privileges.
- **Non-dumpable process.** On Linux, imulup marks itself non-dumpable, so other processes can't read its
  `/proc/<pid>/environ` or memory without `CAP_SYS_PTRACE`.

### Repository content

- `imulup.yaml` must be a regular file. imulup refuses a symlink, which could otherwise point at `/proc/self/environ`
  and leak secrets into a parse error. It also reads at most 1 MiB of it.
- Project directories, `--dir` arguments and `when_modified` globs can't escape the repository.
- Output can't break out of its code block in the PR comment.

## What imulup can't protect

- **Cloud credentials.** Steps need them, so PR code can read them. Scope them tightly: one role per environment,
  least privilege, and short-lived credentials (instance roles, OIDC through Pulumi ESC) instead of long-lived keys.
  See [Cloud credentials](/pulumi/credentials/).
- **Other PRs.** All PRs share one step user. One PR's code can read or tamper with another PR's checkout, or with
  shared caches such as `~/.pulumi/plugins` and package caches. It could, for example, plant a malicious plugin that
  another PR's apply later runs.
- **Your Pulumi state.** Whoever can run a plan can usually read the state backend, including stack outputs.
- **Repository configuration.** `imulup.yaml`, including its workflows, apply requirements and `automerge`, is read
  from the PR's head commit. A PR can change any of it for itself.
- **A leaked app key.** It can mint tokens for every installation of the app. Install the app only on the
  repositories imulup needs.

## Recommendations

1. Run the [Docker images](/install/docker/), or set up
   [privilege separation](/security/privilege-separation/) yourself.
2. Use a [GitHub App](/github/app/), installed only on the repositories imulup manages.
3. Keep `--repo-allowlist` to repositories whose contributors you'd trust with the cloud credentials.
4. Leave `--allow-fork-prs` and `--skip-collaborator-check` off.
5. Give steps short-lived, least-privilege cloud credentials, separate per environment.
6. Expose only `POST /events`, over TLS. See [Running in production](/install/production/).
7. For strong isolation between repositories, run a separate imulup instance per trust boundary, each with its own
   app installation and credentials.

To report a vulnerability, see the
[security policy](https://github.com/imulup/imulup/blob/main/SECURITY.md).
