---
title: Comment commands
description: The syntax of imulup's pull request comment commands.
---

```text
imulup <command> [options]

Commands:
  plan     Run `pulumi preview` for projects changed in this PR (or those selected)
  apply    Run `pulumi up` for planned projects (or those selected)
  unlock   Release this PR's locks and discard its plans
  help     Show this help

Options (plan/apply):
  -p, --project <name>   Project name from imulup.yaml
  -d, --dir <dir>        Project directory, relative to the repo root
  -s, --stack <stack>    Pulumi stack
```

## Parsing rules

- imulup only reads the **first non-empty line** of a comment. You can add an explanation on the following lines.
- The line must start with the command name, `imulup` by default (change it with `--command-name`).
- Arguments are split like a shell command line, so you can quote values: `-d "my dir"`.
- Options take a value as the next word or with `=`: `--stack prod` or `--stack=prod`.
- `imulup`, `imulup help`, `imulup -h` and `imulup --help` all reply with the usage text.
- An unknown command or option, a missing value, or a repeated option gets an error comment with the usage text.

## `plan`

| Form | Plans |
| --- | --- |
| `imulup plan` | Every project the PR modifies, including ones with autoplan turned off |
| `imulup plan -p <name>` | The project with that name in `imulup.yaml` |
| `imulup plan -d <dir>` | Every stack of the project in `<dir>`, whether or not the PR modifies it |
| `imulup plan -d <dir> -s <stack>` | That one stack |
| `imulup plan -s <stack>` | With `imulup.yaml`: every project with that stack. Without: every modified project with that stack. |

`-p` needs an `imulup.yaml`, and can't be combined with `-d` or `-s`. `-d` must be a relative path inside the
repository.

## `apply`

| Form | Applies |
| --- | --- |
| `imulup apply` | Every project with a successful plan on this PR that hasn't been applied yet |
| `imulup apply -p <name>` | That project |
| `imulup apply -d <dir> [-s <stack>]` | The matching projects |

Each project still needs a successful, unapplied plan at the PR's current commit, and has to meet its
[apply requirements](/usage/apply-requirements/). Projects that don't are skipped, with the reason in the
comment.

## `unlock`

```text
imulup unlock
```

Releases every lock the PR holds and discards its plans. It takes no options. See [Locking](/usage/locking/).

## Permissions

Every command except `help` needs write access to the repository, unless the server runs with
`--skip-collaborator-check`. imulup reacts with 👀 to each command it accepts. Commands on closed PRs, and on fork PRs
when `--allow-fork-prs` is off, are refused with a comment.
