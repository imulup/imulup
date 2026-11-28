---
title: Apply requirements
description: Require approval or mergeability before imulup applies a project.
---

Apply requirements are conditions a PR must meet before `imulup apply` runs for a project. They don't affect plans.

| Requirement | Met when |
| --- | --- |
| `approved` | At least one reviewer other than the PR author has approved, and that approval is their latest review |
| `mergeable` | GitHub reports that the PR has no merge conflicts |

## Per project

```yaml
version: 1
projects:
  - name: network-prod
    dir: network
    stack: prod
    apply_requirements: [approved, mergeable]
  - name: network-dev
    dir: network
    stack: dev       # no requirements
```

## For every project

Server flags add requirements to every project in every repository, on top of any in `imulup.yaml`:

```sh
IMULUP_REQUIRE_APPROVAL=true
IMULUP_REQUIRE_MERGEABLE=true
```

A repository can't turn off a requirement the server sets.

:::caution[A PR can edit its own requirements]
imulup reads `imulup.yaml` from the PR's head commit. A PR can therefore delete `apply_requirements` from a project,
and that change applies to the same PR. When a requirement has to hold for every PR, set it on the server with
`IMULUP_REQUIRE_APPROVAL` or `IMULUP_REQUIRE_MERGEABLE`. You can also protect `imulup.yaml` with a `CODEOWNERS` entry
and required code-owner reviews, but that only gates the merge, not the apply.
:::

## When a requirement isn't met

imulup skips the project with a message, and still applies the others:

```text
⏸️ network-prod — skipped
apply requirement `approved` is not met
```

Once the requirement is met, comment `imulup apply` again. The plan stays valid as long as nobody pushes.

## Details and limits

**`approved`**
- imulup reads the PR's reviews and keeps each reviewer's latest review that isn't a plain comment. The requirement is
  met if any of those is an approval, so a later *Request changes* from the same reviewer cancels their approval.
- Reviews from the PR author don't count.
- imulup doesn't check whether the approver has write access, or whether they're a code owner. Enforce that with
  branch protection, and require `imulup/apply` to pass before merging (see below).
- Approvals that GitHub dismissed, for example with *Dismiss stale pull request approvals when new commits are
  pushed*, don't count.

**`mergeable`**
- This is GitHub's `mergeable` flag: no conflicts with the base branch. It doesn't evaluate branch protection
  rules or required checks.
- GitHub computes the flag in the background. Right after a push it can still be unknown, which counts as not met.
  Retry after a few seconds.

## Using branch protection with imulup

Apply requirements gate the apply, and branch protection gates the merge. Together they give the Atlantis-style
"apply before merge" flow:

1. Require reviews (and code owners, if you use them) in branch protection.
2. Require the `imulup/apply` status check, so a PR can't merge until its projects are applied or unchanged.
3. Set `apply_requirements: [approved, mergeable]` on sensitive projects, so nobody can apply without review.
