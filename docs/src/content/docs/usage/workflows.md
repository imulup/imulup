---
title: Workflows
description: Customize the steps imulup runs for plan and apply, and install language dependencies.
---

A workflow is the list of steps imulup runs for `plan` and for `apply`. The built-in workflow is:

```yaml
plan:
  steps:
    - preview
apply:
  steps:
    - up
```

Define your own under `workflows:` in `imulup.yaml`, and select one per project with `workflow:`:

```yaml
version: 1
projects:
  - dir: app
    stack: prod
    workflow: python

workflows:
  python:
    plan:
      steps:
        - run: pulumi install
        - preview:
            extra_args: ["--refresh"]
    apply:
      steps:
        - run: pulumi install
        - up
```

A workflow that leaves out `plan` or `apply` uses the built-in steps for it.

## Steps

| Step | Runs |
| --- | --- |
| `preview` | `pulumi preview --stack <stack> --non-interactive --diff --color never` |
| `up` | `pulumi up --stack <stack> --non-interactive --diff --color never --yes --skip-preview` |
| `preview: {extra_args: [...]}` | `preview`, with the extra arguments appended |
| `up: {extra_args: [...]}` | `up`, with the extra arguments appended |
| `run: <command>` | `sh -c <command>` |

Every step runs in the project directory. Steps run in order, and the first one that fails stops the workflow. The
output of every step, stdout and stderr interleaved, goes into the PR comment. With more than one step, each step's
output starts with a `$ <command>` line.

Each step has a time limit, set by `--step-timeout-secs` (default one hour). A step that runs out of time is killed
and fails.

`run:` steps get a few `IMULUP_*` variables describing the job, such as `IMULUP_STACK` and `IMULUP_PULL`. See the
[step environment](/imulup/reference/step-environment/).

## Installing dependencies

The built-in workflow doesn't install language dependencies. Either install them with a `run:` step, or make sure
they're already in the checkout. Checkouts are reused between runs of the same PR, so ignored directories such as
`node_modules` or a virtualenv persist between plan and apply.

`pulumi install` handles every language: it installs the packages and plugins that `Pulumi.yaml` declares. You can also
use each language's own tools:

| Language | Step |
| --- | --- |
| Node.js | `run: npm ci` (or `yarn install --frozen-lockfile`, `pnpm install --frozen-lockfile`) |
| Python | `run: pulumi install`, with a virtualenv set in `Pulumi.yaml` (see below) |
| Go | Nothing; `pulumi preview` builds the program. `run: go mod download` warms the cache. |
| .NET | Nothing; `pulumi preview` restores and builds. `run: dotnet restore` to do it separately. |
| Java | Nothing for Maven or Gradle projects; Pulumi builds them. |
| YAML | Nothing |

### Python virtualenvs

In the Docker images, steps run as an unprivileged user, so a system-wide `pip install` fails. Declare a virtualenv in
`Pulumi.yaml`, and Pulumi creates and uses it:

```yaml title="Pulumi.yaml"
name: app
runtime:
  name: python
  options:
    toolchain: pip
    virtualenv: venv
```

`pulumi install` then creates `venv/` and installs `requirements.txt` into it. With `toolchain: uv` or `toolchain:
poetry`, Pulumi uses those tools instead.

## Examples

Run tests before previewing:

```yaml
workflows:
  tested:
    plan:
      steps:
        - run: npm ci
        - run: npm test
        - preview
```

Refresh and target a single resource on apply:

```yaml
workflows:
  careful:
    apply:
      steps:
        - up:
            extra_args: ["--refresh", "--target", "urn:pulumi:prod::app::aws:s3/bucket:Bucket::assets"]
```

Use a policy pack:

```yaml
workflows:
  policy:
    plan:
      steps:
        - preview:
            extra_args: ["--policy-pack", "../policy"]
```
