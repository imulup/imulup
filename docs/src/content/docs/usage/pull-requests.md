---
title: The pull request workflow
description: What imulup does at each stage of a pull request, and the commit statuses it sets.
---

## 1. Open or push: autoplan

imulup plans automatically when a PR is opened, reopened, pushed to, or marked ready for review. It plans every
[project and stack](/imulup/usage/projects/) the PR modifies, and posts one comment with a section for each:

```text
imulup plan — 2/2 projects succeeded

✅ network-dev — changes pending
   ▸ Output                          (collapsed; the pulumi preview diff)
   • To apply: imulup apply -p network-dev
   • To re-plan: imulup plan -p network-dev

✅ network-prod — no changes
   ▸ Output
   • To apply: imulup apply -p network-prod
   • To re-plan: imulup plan -p network-prod
───
• Apply all planned projects: imulup apply
• Discard plans and release locks: imulup unlock
```

The output is a `diff` code block, so GitHub highlights additions, deletions and updates. It's collapsed for
successful projects and expanded for failed ones. Long output is cut from the top, keeping Pulumi's summary and
errors at the end.

Autoplan skips:

- draft PRs, unless the server runs with `--plan-drafts`
- PRs from forks, unless the server runs with `--allow-fork-prs`
- projects with `autoplan: {enabled: false}` in `imulup.yaml`
- everything, if the server runs with `--disable-autoplan`

If the PR doesn't modify any Pulumi project, imulup doesn't comment.

## 2. Review the plan

A plan is tied to the commit it ran on. Every push triggers a new autoplan, which replaces the earlier plans. With
autoplan off, comment `imulup plan` after pushing.

To plan again without pushing, for example after fixing something outside the repo, comment:

```text
imulup plan
```

See [Comment commands](/imulup/reference/commands/) for how to plan only some projects.

## 3. Apply

```text
imulup apply
```

With no options, `apply` applies every project with a successful plan that hasn't been applied yet. To apply one:

```text
imulup apply -p network-dev
imulup apply -d network -s dev
```

imulup skips a project, and says why in the comment, if:

- it has no plan, or its plan is for an older commit than the PR's head
- its last plan failed
- it has already been applied at this commit
- another PR holds its [lock](/imulup/usage/locking/)
- its [apply requirements](/imulup/usage/apply-requirements/) aren't met

The other selected projects still run.

## 4. Merge

Merge the PR as usual, or let [automerge](/imulup/usage/automerge/) do it once everything is applied. When the PR is
merged or closed, imulup releases its locks, discards its plans, deletes its checkout, and comments with the list of
released locks.

## Who can run commands

Commenters need write access (*write*, *maintain* or *admin*) to the repository. That applies to `plan` and `unlock`
as well as `apply`. Without it, imulup replies that the user needs write access. `--skip-collaborator-check` lets anyone
who can comment run commands. Only use it on private repositories with trusted members.

When imulup accepts a command, it reacts to the comment with 👀. Comments from imulup's own account are always
ignored.

## Commit statuses

imulup sets two statuses on the PR's head commit:

| Context | State | Description |
| --- | --- | --- |
| `imulup/plan` | pending | `planning N project(s)` |
| | success | `N planned` |
| | failure | `X/N failed`, `setup failed`, or `plans discarded by unlock` |
| `imulup/apply` | pending | `X/N applied or unchanged` |
| | success | every successful plan at this commit is applied or has no changes |
| | failure | an apply failed |

To make "apply, then merge" the rule, mark `imulup/apply` as a required status check in your branch protection or
ruleset. GitHub then won't merge the PR until every planned project is applied or unchanged.

`imulup/apply` only appears once a plan has succeeded. If you make it required, a PR that doesn't touch any Pulumi
project never gets the status, and can't be merged. Either scope the rule to the branches and paths that need it,
or let maintainers bypass it.
