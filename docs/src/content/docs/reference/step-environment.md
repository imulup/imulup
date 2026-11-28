---
title: Step environment
description: The working directory, environment variables and limits that workflow steps run with.
---

## Working directory

Every step runs in the project's directory inside the PR checkout, `<data-dir>/repos/<owner>/<repo>/<pr>/<dir>`.
The checkout is the PR's head commit, cleaned of untracked files that aren't ignored. Ignored files, such as
`node_modules`, survive between runs.

## Environment variables

imulup sets these for every step:

| Variable | Example | Description |
| --- | --- | --- |
| `IMULUP_STACK` | `prod` | The stack |
| `IMULUP_PROJECT` | `network-prod` | The project's name, or `dir:stack` |
| `IMULUP_DIR` | `network` | The project directory, relative to the repo root |
| `IMULUP_REPO` | `my-org/infra` | The repository |
| `IMULUP_PULL` | `42` | The PR number |
| `IMULUP_HEAD_SHA` | `3f2c…` | The commit being planned or applied |
| `IMULUP_USER` | `alice` | Who triggered the run: the commenter, or the PR author for autoplan |
| `PULUMI_SKIP_UPDATE_CHECK` | `true` | Stops the Pulumi CLI from checking for updates |
| `CI` | `true` | Tells tools they're running non-interactively |

With privilege separation, `HOME`, `USER` and `LOGNAME` are set to the step user's.

Every other variable in imulup's environment passes through, except imulup's own `IMULUP_*` configuration. That
covers the GitHub token, app key and webhook secret.

## Input and output

- stdin is empty (`/dev/null`).
- stdout and stderr are merged, in order, and posted to the PR with ANSI color codes removed.
- GitHub tokens and the webhook secret are replaced with `***` in the output.

## Limits

- Each step can run for `--step-timeout-secs` seconds, one hour by default. imulup kills a step that runs longer and
  fails it with `step timed out after Ns`.
- A PR comment holds up to about 60,000 characters. Longer output is cut from the top, with a
  `… N bytes truncated …` marker, so Pulumi's summary and errors at the end stay visible.

## Change detection

imulup reads the `Resources:` summary at the end of `pulumi preview` output to decide whether a plan has changes. It
looks for `to create`, `to update`, `to delete`, `to replace` or `to import`. The result drives the "changes pending"
and "no changes" labels, the `imulup/apply` status and automerge. If your workflow's last step hides or changes that
summary, imulup treats the plan as having unknown changes, and expects it to be applied.
