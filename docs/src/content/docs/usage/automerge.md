---
title: Automerge
description: Have imulup merge a pull request once everything it planned has been applied.
---

With automerge on, imulup merges a PR as soon as every project it planned at the PR's current commit is either
applied or has no changes.

## Turning it on

For every repository:

```sh
IMULUP_AUTOMERGE=true
IMULUP_AUTOMERGE_METHOD=squash   # merge (default), squash or rebase
```

Or per repository, in `imulup.yaml`, which overrides the server setting either way:

```yaml
version: 1
automerge: true
```

`automerge: false` turns it off for a repository even when the server has it on.

Automerge needs **Contents: Read and write**. For a GitHub App, change the permission in the app settings and accept
the new permission on each installation. GitHub doesn't apply it until you do. A token needs the same.

## When imulup merges

imulup checks after each successful `imulup apply`. It merges when:

- at least one project was planned at the PR's current head commit,
- every one of those plans succeeded, and
- every one is applied, or its preview showed no changes.

If any plan failed, or only some projects are applied, the PR stays open. Applying the rest later triggers the merge.

The merge is pinned to the commit imulup planned and applied. If someone pushes in between, GitHub refuses the merge.

## When the merge fails

GitHub can refuse the merge, for example because branch protection still requires a review, a required check is
failing, or the merge method isn't allowed in the repository settings. imulup then comments:

```text
⚠️ Automerge failed

Every planned project was applied, but the merge failed:

GitHub refused the merge (405 Method Not Allowed): Required status check "test" is expected.

Merge the PR yourself once that's resolved.
```

imulup doesn't retry. Merge the PR yourself, or comment `imulup apply` again once the problem is fixed. That only
works if a project is still waiting to apply; otherwise just merge it.

## Branch protection still applies

imulup merges through the normal GitHub API as the app or token user. Required reviews, required checks and merge
queues all still apply. To let imulup merge despite a rule, add the app as a bypass actor in the ruleset, but think
about whether that weakens the rule you wanted.

Note that `imulup/apply` is set to success just before the merge, so requiring it doesn't block automerge.
