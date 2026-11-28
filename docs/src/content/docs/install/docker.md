---
title: Docker
description: The imulup container images, and running them with Docker Compose.
---

Docker is the recommended way to run imulup. The images include the Pulumi CLI and language runtimes, and they
come with [privilege separation](/security/privilege-separation/) already set up.

## Images

Images are published to `ghcr.io/imulup/imulup` for `linux/amd64` and `linux/arm64`. There is a "full" variant with
every Pulumi language runtime, and a slim variant per language:

| Tag | Base | Size (arm64) |
| --- | --- | --- |
| `<version>`, `latest` | `pulumi/pulumi` (all languages) | ~14 GB |
| `<version>-nodejs`, `nodejs` | `pulumi/pulumi-nodejs` | ~1.2 GB |
| `<version>-python`, `python` | `pulumi/pulumi-python` | ~0.8 GB |
| `<version>-go`, `go` | `pulumi/pulumi-go` | ~0.8 GB |
| `<version>-dotnet`, `dotnet` | `pulumi/pulumi-dotnet` | ~1.4 GB |
| `<version>-java`, `java` | `pulumi/pulumi-java` | ~1.8 GB |

`<version>` has no `v` prefix, for example `0.4.0-python`. Pin a version tag in production so an upgrade only
happens when you choose it.

Every variant uses the same Pulumi CLI version. Pulumi YAML programs work in any variant.

### What's in the image

- `/usr/local/bin/imulup` is the entrypoint. Pass flags as arguments, or set `IMULUP_*` environment variables.
- `IMULUP_DATA_DIR=/data` is declared as a volume. It holds the lock state, git mirrors and PR checkouts.
- Port `4141` is exposed.
- There are two users:
  - `imulup` (uid 10001) runs the server.
  - `imulup-runner` (uid 10002, home `/home/imulup-runner`) runs checkouts and workflow steps.

  The image sets `IMULUP_SERVER_USER` and `IMULUP_STEP_USER` to these users. The container must start as root:
  imulup reads its secrets and prepares `/data`, and then drops to `imulup`.

Steps run as `imulup-runner`, so they can't install packages system-wide. Install dependencies into the project or
the runner's home directory instead: a virtualenv, `pip install --user`, a local `node_modules`, and so on. See
[Workflows](/usage/workflows/).

## Docker Compose

The repository has a ready-made Compose setup in [`deploy/`](https://github.com/imulup/imulup/tree/main/deploy):

```sh
cp deploy/.env.example deploy/.env     # fill in the values
cp ~/Downloads/your-app.private-key.pem deploy/github-app.pem
chmod 600 deploy/github-app.pem
docker compose -f deploy/compose.yaml up -d
```

`compose.yaml` looks like this:

```yaml
services:
  imulup:
    image: ghcr.io/imulup/imulup:python
    restart: unless-stopped
    env_file: .env
    environment:
      IMULUP_DATA_DIR: /data
      RUST_LOG: ${RUST_LOG:-info}
    ports:
      - "${IMULUP_PUBLISH_PORT:-4141}:4141"
    volumes:
      - imulup-data:/data
    secrets:
      - gh_app_key
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://localhost:4141/healthz"]
      interval: 30s
      timeout: 5s
      retries: 3
      start_period: 10s

volumes:
  imulup-data:

secrets:
  gh_app_key:
    file: ./github-app.pem
```

And this is the minimum `.env` for a GitHub App:

```sh
IMULUP_GH_APP_ID=123456
IMULUP_GH_APP_KEY_FILE=/run/secrets/gh_app_key
IMULUP_GH_WEBHOOK_SECRET=...
IMULUP_REPO_ALLOWLIST=my-org/infra
PULUMI_ACCESS_TOKEN=...
```

:::caution
The `secrets:` block only mounts the key file at `/run/secrets/gh_app_key`. You still have to point imulup at it with
`IMULUP_GH_APP_KEY_FILE`. Without that variable, imulup exits with
`the following required arguments were not provided: <--gh-app-key …|--gh-app-key-file …>`.
:::

:::caution
Don't leave unused options set to an empty value, such as `IMULUP_GH_TOKEN=`. An empty variable still counts as set,
so it conflicts with `IMULUP_GH_APP_ID`. Comment out the lines you don't use.
:::

If you authenticate with a [token](/github/token/) instead, set `IMULUP_GH_TOKEN` and `IMULUP_GH_USER`, and
remove both `secrets:` blocks.

To put imulup behind a reverse proxy on the same host, publish it on loopback only with
`IMULUP_PUBLISH_PORT=127.0.0.1:4141`. See [Running in production](/install/production/).

## `docker run`

```sh
docker run -d --name imulup -p 4141:4141 \
  -e IMULUP_GH_APP_ID -e IMULUP_GH_WEBHOOK_SECRET -e IMULUP_REPO_ALLOWLIST -e PULUMI_ACCESS_TOKEN \
  -e IMULUP_GH_APP_KEY_FILE=/run/secrets/app.pem \
  -v /etc/imulup/app.pem:/run/secrets/app.pem:ro \
  -v imulup-data:/data \
  ghcr.io/imulup/imulup:0.4.0-python
```

## Building the images

The images are built from one `Dockerfile` with [Bake](https://docs.docker.com/build/bake/). `PULUMI_VERSION` in
`docker-bake.hcl` pins the Pulumi CLI for every variant.

```sh
docker buildx bake --load                # all variants for the local platform
docker buildx bake --load nodejs         # just one
VERSION=0.4.0 REGISTRY=ghcr.io/acme docker buildx bake --push \
  --set '*.platform=linux/amd64,linux/arm64'
```

To add tools your workflows need, such as a cloud CLI, build on top of a published image:

```dockerfile
FROM ghcr.io/imulup/imulup:0.4.0-nodejs
RUN apt-get update && apt-get install -y --no-install-recommends awscli \
    && rm -rf /var/lib/apt/lists/*
```
