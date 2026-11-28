---
title: FAQ
description: Common questions about imulup.
---

### Does imulup support GitLab, Bitbucket or Azure DevOps?

Not yet. imulup supports GitHub and GitHub Enterprise Server only. The VCS code sits behind one trait (`vcs::Vcs`),
so adding other providers is possible. Contributions are welcome.

### Is there a Helm chart?

No. imulup is one container with one volume. To run it on Kubernetes, use a Deployment with `replicas: 1` and the
`Recreate` strategy, a PersistentVolumeClaim mounted at `/data`, and the app key from a Secret with
`defaultMode: 0400`. See [Running in production](/imulup/install/production/#one-instance-only).

### Can I run more than one replica?

No. Locks and plans live in one JSON file with no coordination between processes. Run a single instance.

### Does `apply` run exactly the plan I reviewed?

It runs `pulumi up` against the same commit the plan ran on, and refuses if the PR has moved since. Resources can
still change between the preview and the update, for example through drift or another tool. imulup doesn't use
Pulumi's experimental saved plans (`--save-plan` / `--plan`).

### Why did my PR plan without anyone reviewing it?

Autoplan runs when a PR is opened or pushed to, and that runs the PR's code. That's why who can open PRs matters. See
the [security model](/imulup/security/model/). To plan only on request, set `--disable-autoplan`, or
`autoplan: {enabled: false}` per project. Then someone with write access has to comment `imulup plan`, but nobody
reviews the code before it runs that way either.

### Can I use a different comment prefix, like `atlantis`?

Yes: `--command-name atlantis`. Comments then start with `atlantis plan`. The PR comment headings and status contexts
still say `imulup`.

### Can imulup create stacks?

Not by itself, but a workflow step can: `run: pulumi stack select --create "$IMULUP_STACK"`. See
[Stack selection](/imulup/pulumi/stacks/#stacks-must-exist).

### Does imulup work with Pulumi ESC?

Yes. Stacks that import ESC environments open them during `preview` and `up`, using `PULUMI_ACCESS_TOKEN`. See
[Cloud credentials](/imulup/pulumi/credentials/#pulumi-esc).

### Can I run `pulumi refresh` or `pulumi destroy`?

Not as commands. You can add them to a workflow as `run:` steps, for example `run: pulumi refresh --yes --stack
"$IMULUP_STACK"`. Be careful with `destroy`: it runs whenever that workflow runs.

### Does imulup expose metrics?

Not yet. `GET /healthz` is the health check. See [HTTP endpoints](/imulup/reference/http/).

### Where does imulup keep Pulumi state?

It doesn't. Pulumi state is in your backend: Pulumi Cloud, S3 and so on. imulup only keeps its locks and plan
records in `<data-dir>/state.json`.
