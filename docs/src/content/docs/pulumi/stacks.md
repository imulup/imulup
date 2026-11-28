---
title: Stack selection
description: How imulup decides which stacks to plan, and how to create stacks on first use.
---

imulup always runs Pulumi with an explicit `--stack`. The stack name comes from one of two places:

- With an [`imulup.yaml`](/reference/repo-config/), it's each project's `stack:`.
- Without one, imulup finds the Pulumi project that a modified file belongs to, and plans every stack with a
  `Pulumi.<stack>.yaml` file next to its `Pulumi.yaml`. A project with `Pulumi.dev.yaml` and `Pulumi.prod.yaml` gets
  two plans, `dev` and `prod`.

See [Projects and stacks](/usage/projects/) for how imulup matches modified files to projects.

## Qualified stack names

With Pulumi Cloud, an unqualified name such as `dev` uses the token's default organization. To use another
organization, list the stack in `imulup.yaml` with its full name:

```yaml
projects:
  - dir: network
    stack: my-org/dev
```

Pulumi still reads `Pulumi.dev.yaml` for its config.

## Stacks must exist

`pulumi preview` fails if the stack doesn't exist yet. Create stacks ahead of time with `pulumi stack init`. To have
imulup create them on first use, add a step to the plan workflow:

```yaml
workflows:
  default:
    plan:
      steps:
        - run: pulumi stack select --create "$IMULUP_STACK"
        - preview
```

Each project also needs `workflow: default`. A project without a `workflow:` uses the built-in workflow, which only
runs `preview`.

## Refreshing

`imulup plan` runs `pulumi preview` without refreshing, by default. To refresh state from the cloud first, pass the
flag through `extra_args`:

```yaml
workflows:
  refresh:
    plan:
      steps:
        - preview:
            extra_args: ["--refresh"]
    apply:
      steps:
        - up:
            extra_args: ["--refresh"]
```
