---
title: Locking
description: How imulup stops two pull requests from changing the same stack at once.
---

Two PRs that change the same stack shouldn't both be applied: the second one would undo the first, or apply on top of
state its plan never saw. imulup prevents this with locks.

## How locks work

- A lock covers one `(repository, directory, stack)`. `network` with `dev` and `network` with `prod` are separate
  locks.
- A PR takes the lock when it plans that project, and keeps it until the PR is merged or closed, or until someone
  comments `imulup unlock` on it. Applying doesn't release the lock: the change isn't on the default branch until the
  PR merges.
- While another PR holds the lock, plans and applies of that project are skipped, with a message naming the PR that
  holds it:

  ```text
  ⏸️ network-prod — skipped
  `network-prod` is locked by #42 (locked by alice). Merge or close that PR, or comment `imulup unlock` on it.
  ```

- The other projects in the same PR still run.

## Releasing locks

| Event | Effect |
| --- | --- |
| PR merged or closed | Releases all of the PR's locks, discards its plans, and deletes its checkout. imulup comments with the released locks. |
| `imulup unlock` on the PR | Releases all of the PR's locks and discards its plans. The `imulup/plan` status turns to failure, `plans discarded by unlock`. |

`unlock` only acts on the PR it's commented on. To free a lock another PR holds, comment `imulup unlock` on that PR.
It takes write access, like every command.

After an `unlock`, the PR has to plan again before it can apply, and the next plan takes the locks again.

## Viewing locks

`GET /` on the imulup server shows a table of every held lock, and `GET /api/locks` returns the same as JSON:

```json
[
  {
    "repo": "my-org/infra",
    "dir": "network",
    "stack": "prod",
    "project": "network-prod",
    "pull": 42,
    "user": "alice",
    "created_at": 1764201600
  }
]
```

`created_at` is a Unix timestamp. These endpoints have no authentication: see
[Running in production](/install/production/#expose-only-the-webhook).

## Locks and Pulumi's own locking

Pulumi backends also lock a stack while an update runs, but only for the duration of one `pulumi up`. imulup's
locks last for the whole life of a PR, so a stack's plan and apply can't be interleaved with another PR's.

Changes made outside imulup, such as a `pulumi up` from a laptop or another CI system, don't see imulup's locks.
