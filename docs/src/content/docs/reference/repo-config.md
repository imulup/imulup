---
title: imulup.yaml
description: The schema of the per-repository imulup.yaml file.
---

`imulup.yaml` (or `imulup.yml`) lives at the root of the repository. It's optional: without it, imulup
[autodetects projects](/imulup/usage/projects/#without-imulupyaml).

imulup reads the file from the PR's head commit, so a PR can change it and the change applies to that PR.

## Example

```yaml
version: 1
automerge: true

projects:
  - name: network-prod
    dir: network
    stack: prod
    apply_requirements: [approved, mergeable]
    autoplan:
      enabled: true
      when_modified: ["**/*.ts", "Pulumi*.yaml", "package*.json", "../shared/**"]
    workflow: node
  - dir: network
    stack: dev
    workflow: node

workflows:
  node:
    plan:
      steps:
        - run: npm ci
        - preview:
            extra_args: ["--refresh"]
    apply:
      steps:
        - run: npm ci
        - up
```

## Top level

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `version` | integer | *required* | Must be `1`. |
| `automerge` | boolean | the server's `--automerge` | Merge PRs once every planned project is applied. See [Automerge](/imulup/usage/automerge/). |
| `projects` | list of [projects](#projects) | `[]` | The projects imulup manages in this repo. With an `imulup.yaml`, only these are planned. |
| `workflows` | map of name → [workflow](#workflows) | `{}` | Named workflows that projects can use. |

## `projects`

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `name` | string | `dir:stack` | Short name for `-p`. Must be unique, can't contain whitespace, and can't start with `-`. |
| `dir` | string | *required* | Directory with `Pulumi.yaml`, relative to the repo root. Use `.` for the root. Can't escape the repository. |
| `stack` | string | *required* | Stack name, passed as `--stack`. Can be fully qualified, such as `my-org/prod`. |
| `apply_requirements` | list | `[]` | Any of `approved` and `mergeable`. See [Apply requirements](/imulup/usage/apply-requirements/). |
| `autoplan.enabled` | boolean | `true` | Plan automatically when the PR modifies the project. |
| `autoplan.when_modified` | list of globs | `["**/*"]` | Files that count as modifying the project, relative to `dir`. `../` is allowed as long as the result stays inside the repository. |
| `workflow` | string | built-in | Name of a workflow under `workflows:`. |

Each `dir` and `stack` pair can appear only once.

Globs use `*` for any characters within one path segment, `**` for any number of segments, `?` for one character, and
`{a,b}` for alternatives. There's no negation.

## `workflows`

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `plan.steps` | list of [steps](#steps) | `[preview]` | Steps for `imulup plan` and autoplan. |
| `apply.steps` | list of steps | `[up]` | Steps for `imulup apply`. |

### Steps

| Step | Description |
| --- | --- |
| `preview` | `pulumi preview --stack <stack> --non-interactive --diff --color never` |
| `up` | `pulumi up --stack <stack> --non-interactive --diff --color never --yes --skip-preview` |
| `preview: {extra_args: [...]}` | `preview` with extra arguments appended |
| `up: {extra_args: [...]}` | `up` with extra arguments appended |
| `run: <command>` | A shell command, run with `sh -c` |

Steps run in the project directory, in order, and stop at the first failure. See [Workflows](/imulup/usage/workflows/)
and the [step environment](/imulup/reference/step-environment/).

## Validation

imulup rejects the file, and comments on the PR with the error, if:

- it has an unknown key, such as `apply_requirement:` instead of `apply_requirements:`
- `version` isn't `1`
- a project uses a `workflow` that isn't defined
- a `dir`/`stack` pair or a `name` appears twice
- a `dir` or `when_modified` glob escapes the repository
- a step isn't one of those above
- it's a symlink rather than a regular file
