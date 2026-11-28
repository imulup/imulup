---
title: Projects and stacks
description: How imulup finds the Pulumi projects and stacks a pull request changes, with or without imulup.yaml.
---

imulup works on **projects**. A project is one directory with a `Pulumi.yaml`, plus one stack: `network` with `dev`
is a different project from `network` with `prod`. Each project is planned, applied and locked on its own.

## Without `imulup.yaml`

Repositories don't need any imulup configuration. For each file the PR modifies, imulup walks up the directory tree
to the nearest `Pulumi.yaml`. It then plans every stack that has a `Pulumi.<stack>.yaml` in that directory.

```text
.
├── network/
│   ├── Pulumi.yaml
│   ├── Pulumi.dev.yaml
│   ├── Pulumi.prod.yaml
│   └── index.ts          ← modified: plans network:dev and network:prod
└── dns/
    ├── Pulumi.yaml
    ├── Pulumi.prod.yaml
    └── zones.ts
```

Autodetected projects are named `dir:stack`, such as `network:dev`, and run the default workflow: just
`pulumi preview` and `pulumi up`, with no dependency install. A stack that has no `Pulumi.<stack>.yaml` (because all
its config is in Pulumi Cloud, say) isn't detected. List it in `imulup.yaml` instead.

## With `imulup.yaml`

Put an `imulup.yaml` (or `imulup.yml`) at the repository root to list projects explicitly. Once it exists, only the
listed projects are planned. A file with `version: 1` and no `projects:` plans nothing.

```yaml
version: 1
projects:
  - name: network-dev
    dir: network
    stack: dev
    workflow: node

  - name: network-prod
    dir: network
    stack: prod
    workflow: node
    apply_requirements: [approved, mergeable]
    autoplan:
      when_modified:
        - "**/*.ts"
        - "Pulumi.yaml"
        - "Pulumi.prod.yaml"
        - "package*.json"
        - "../shared/**"

  - name: dns
    dir: dns
    stack: prod
    autoplan:
      enabled: false

workflows:
  node:
    plan:
      steps:
        - run: npm ci
        - preview
    apply:
      steps:
        - run: npm ci
        - up
```

- **`name`** is optional. It gives the project a short name for `-p` in comments. Without it, the project is
  `dir:stack`.
- **`dir`** is relative to the repo root. `.` is the root itself.
- **`autoplan.when_modified`** decides whether a PR modifies the project. The globs are relative to `dir` and
  default to `["**/*"]`. They can reach outside it, like `../shared/**`, but not outside the repository.
- **`autoplan.enabled: false`** stops automatic plans for the project. A bare `imulup plan` still plans it if it's
  modified, and so does `imulup plan -p dns`.
- **`workflow`** names a workflow under `workflows:`. See [Workflows](/usage/workflows/).

The full schema is in the [`imulup.yaml` reference](/reference/repo-config/).

## Several stacks and projects in one PR

A PR that touches several projects gets one plan comment covering all of them. Projects run one after another, and
each takes its own [lock](/usage/locking/), so one locked or failing project doesn't stop the rest.

`imulup apply` with no options then applies every project with a successful plan. To apply in a set order, such as
`dev` before `prod`, apply them one at a time:

```text
imulup apply -p network-dev
imulup apply -p network-prod
```

## Monorepos

For a repository with many projects, `imulup.yaml` keeps plans focused:

- Use narrow `when_modified` globs so a change to one service doesn't plan everything.
- Point shared code at its users with globs like `../lib/**`.
- Set `autoplan: {enabled: false}` on rarely changed or expensive stacks, and plan them on demand.
