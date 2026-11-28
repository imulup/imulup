---
title: Upgrade notes
description: What changed in each imulup release, and what to do when you upgrade.
---

imulup follows [semantic versioning](https://semver.org/). Before 1.0, minor releases can change behavior. Read the
notes for every version you skip. Each version is a [tag](https://github.com/imulup/imulup/tags) in the repository.

To upgrade a Docker deployment, change the image tag and recreate the container. State in `/data` carries over.

```sh
docker compose -f deploy/compose.yaml pull
docker compose -f deploy/compose.yaml up -d
```

## 0.4.0

- **Fixed:** `pulumi install` and other tools that list parent directories failed with
  `open /data: permission denied`. The data directory is now mode `0755`, so the step user can list it.
  `state.json` stays `0600`.

Nothing to do: imulup fixes the mode at startup.

## 0.3.0

- **New:** [automerge](/usage/automerge/). imulup can merge a PR once every planned project is applied,
  with `--automerge`, `--automerge-method`, or `automerge:` in `imulup.yaml`.

Automerge is off by default. To use it with a GitHub App, change **Contents** to *Read and write*, and accept the new
permission on each installation.

## 0.2.0

This release changes how imulup runs. Read it before upgrading from 0.1.

- **New:** [GitHub App](/github/app/) authentication, with `--gh-app-id` and `--gh-app-key-file`, using
  per-repository, least-privilege installation tokens.
- **New:** [privilege separation](/security/privilege-separation/). The Docker images now run the server
  as `imulup` and PR code as `imulup-runner`.
- **Changed:** imulup refuses to start if the app key file is readable by group or others.
- **Changed:** imulup refuses an `imulup.yaml` that is a symlink.
- **Changed:** imulup marks itself non-dumpable on Linux.
- **Changed:** `--gh-user` is optional with a GitHub App. It defaults to `<app-slug>[bot]`.

When upgrading:

1. **Steps no longer run as root in the images.** Workflows that install packages system-wide (`pip install`,
   `npm install -g`, `apt-get`) fail. Use a virtualenv or project-local installs (see
   [Workflows](/usage/workflows/#installing-dependencies)), or bake tools into a custom image.
2. **Files that steps need must be readable by `imulup-runner`** (uid 10002), for example a mounted cloud credentials
   file.
3. **The container must start as root.** If you run it with `--user`, or `runAsUser` in Kubernetes, remove that.
   To keep the old behavior, set `IMULUP_SERVER_USER=` and `IMULUP_STEP_USER=` to empty values. That isn't
   recommended.
4. **Existing data is migrated.** imulup re-owns the existing `/data` contents for the new users at startup.

## 0.1.0

The first release.
