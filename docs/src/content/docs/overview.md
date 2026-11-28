---
title: Overview
description: What imulup does, how it compares with Atlantis and Pulumi Deployments, and how a request flows through it.
---

imulup is a small server that turns pull requests into the review and deploy step for [Pulumi](https://www.pulumi.com/)
infrastructure. It listens for GitHub webhooks, runs `pulumi preview` for each project and stack a PR changes, and
posts the result as a PR comment. When the diff looks right, someone with write access comments `imulup apply` and
imulup runs `pulumi up`.

It's modelled on [Atlantis](https://www.runatlantis.io/), which does the same for Terraform.

## How a pull request flows through imulup

```text
 GitHub                         imulup                                 Pulumi
 ──────                         ──────                                 ──────
 PR opened / pushed ──webhook──▶ POST /events
                                  │ verify X-Hub-Signature-256
                                  │ check repo allowlist
                                  ▼
                                 fetch PR head into a mirror,
                                 check it out for this PR
                                  │
                                  ▼
                                 find modified projects/stacks
                                 (imulup.yaml or Pulumi.yaml)
                                  │
                                  ▼
                                 lock (repo, dir, stack) ─────────────▶ pulumi preview --diff
                                  │                                        │
 PR comment: plan diff ◀──────────┤◀────────────────────── output ─────────┘
 status: imulup/plan
                                  ⋮
 "imulup apply" comment ─webhook─▶ check write access, current plan,
                                   apply requirements ────────────────▶ pulumi up --yes
                                  │                                        │
 PR comment: apply output ◀───────┤◀───────────────────── output ──────────┘
 status: imulup/apply              │
                                  ▼
                                 (optional) merge the PR
                                  ⋮
 PR merged / closed ──webhook───▶ release locks, delete checkout
```

1. **Webhook.** GitHub sends `pull_request` and `issue_comment` events to `POST /events`. imulup rejects any request
   without a valid HMAC signature, and ignores repos that aren't on the allowlist.
2. **Checkout.** imulup fetches the PR head, with credentials, into a bare mirror it owns. It then checks out a
   per-PR working tree from that mirror. With privilege separation on, the working tree belongs to a separate,
   unprivileged user.
3. **Discovery.** imulup works out which Pulumi projects and stacks the PR changes. It reads `imulup.yaml` if the
   repo has one, or otherwise walks up from each modified file to the nearest `Pulumi.yaml`.
4. **Plan.** For each project, imulup takes a lock on `(repo, dir, stack)`. It then runs the project's plan
   workflow (by default just `pulumi preview`) and comments with the diff.
5. **Apply.** On `imulup apply`, imulup checks that the commenter has write access, that a successful plan exists
   for the PR's current head commit, and that the project's apply requirements are met. It then runs `pulumi up`.
6. **Close.** When the PR is merged or closed, imulup releases its locks and deletes its checkout.

## Compared with Atlantis

| Atlantis | imulup |
| --- | --- |
| `atlantis plan` → `terraform plan` | `imulup plan` → `pulumi preview --diff` |
| `atlantis apply` → `terraform apply` | `imulup apply` → `pulumi up --yes --skip-preview` |
| directory + workspace | directory + **stack** |
| `atlantis.yaml` | `imulup.yaml` |
| GitHub, GitLab, Bitbucket, Azure DevOps | GitHub (and GitHub Enterprise Server) |

Some differences go beyond naming:

- **Plans aren't saved.** Terraform can save a plan file and apply exactly that plan. imulup instead re-runs
  `pulumi up` against the same commit the plan ran on, and refuses to apply if the PR has moved since. It doesn't
  use Pulumi's experimental `--save-plan` / `--plan`.
- **Programs are code.** A Terraform plan evaluates HCL. A Pulumi preview runs your program in Node.js, Python, Go,
  .NET or Java, so anyone who can open a PR can run code on the imulup host. See the
  [security model](/imulup/security/model/).
- **Single instance.** State is one JSON file, so you run one imulup server.

## Compared with Pulumi Deployments

[Pulumi Deployments](https://www.pulumi.com/docs/pulumi-cloud/deployments/) is Pulumi Cloud's managed service for
running Pulumi operations, including review stacks and previews on pull requests. Choose by where you want the
code to run and who holds the credentials:

- **imulup** runs on your own infrastructure and works with any Pulumi backend, including S3, GCS, Azure Blob and
  local state. Cloud credentials stay on a host you control.
- **Pulumi Deployments** is managed for you and integrated with Pulumi Cloud. It runs on Pulumi's infrastructure
  or on self-hosted agents, and needs Pulumi Cloud for state.

## What's next

- Try it out in about ten minutes with the [quickstart](/imulup/quickstart/).
- Read the [security model](/imulup/security/model/) before you point imulup at real cloud credentials.
