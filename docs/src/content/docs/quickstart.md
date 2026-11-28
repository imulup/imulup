---
title: Quickstart
description: Run imulup on your machine against a test repo, with no cloud account, in about ten minutes.
---

This walkthrough runs imulup in Docker on your machine and points a test repository at it. The sample Pulumi
program only uses the `random` provider and keeps its state on local disk, so you don't need a cloud account or
Pulumi Cloud.

You'll need:

- Docker
- A GitHub account where you can create a repository and a GitHub App
- A way to expose a local port over HTTPS, such as
  [`cloudflared`](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/do-more-with-tunnels/trycloudflare/)
  or [ngrok](https://ngrok.com/)

## 1. Create a test repository

Create a new repository, for example `you/imulup-test`, with these three files on `main`.

`quickstart/Pulumi.yaml`:

```yaml
name: quickstart
runtime: yaml
resources:
  pet:
    type: random:RandomPet
outputs:
  name: ${pet.id}
```

`quickstart/Pulumi.dev.yaml`:

```yaml
config: {}
```

`imulup.yaml`:

```yaml
version: 1
projects:
  - dir: quickstart
    stack: dev
    workflow: quickstart
workflows:
  quickstart:
    plan:
      steps:
        # The local backend starts empty, so create the stack on first use.
        - run: pulumi stack select --create "$IMULUP_STACK"
        - preview
    apply:
      steps:
        - up
```

## 2. Start a tunnel

imulup listens on port 4141. Expose it so GitHub can reach it:

```sh
cloudflared tunnel --url http://localhost:4141
```

Note the `https://….trycloudflare.com` URL it prints. Leave the tunnel running.

## 3. Create a GitHub App

Go to **Settings → Developer settings → GitHub Apps → New GitHub App** and fill in:

- **Webhook URL:** your tunnel URL followed by `/events`, for example `https://example.trycloudflare.com/events`
- **Webhook secret:** a random value. Generate one with `openssl rand -hex 32` and keep it for step 4.
- **Repository permissions:**
  - Contents: *Read-only*
  - Pull requests: *Read and write*
  - Issues: *Read and write*
  - Commit statuses: *Read and write*
  - Metadata: *Read-only* (selected automatically)
- **Subscribe to events:** *Pull request* and *Issue comment*
- **Where can this GitHub App be installed?** *Only on this account*

Create the app and note its **App ID**. Then:

1. Under **Private keys**, choose **Generate a private key**. Save the downloaded file as `app.pem` and run
   `chmod 600 app.pem`. imulup refuses to start if group or others can read the key.
2. Choose **Install App**, and install it on your test repository only.

## 4. Run imulup

```sh
docker run --rm -p 4141:4141 \
  -e IMULUP_GH_APP_ID=123456 \
  -e IMULUP_GH_APP_KEY_FILE=/run/secrets/app.pem \
  -e IMULUP_GH_WEBHOOK_SECRET=the-secret-from-step-3 \
  -e IMULUP_REPO_ALLOWLIST=you/imulup-test \
  -e PULUMI_BACKEND_URL=file://~ \
  -e PULUMI_CONFIG_PASSPHRASE=quickstart \
  -v "$PWD/app.pem:/run/secrets/app.pem:ro" \
  -v imulup-quickstart:/data \
  ghcr.io/imulup/imulup:python
```

Replace the app ID, secret and repository with your own. Any image variant works, because the Pulumi YAML runtime is
in all of them. The `python` variant is one of the smallest.

You should see:

```text
INFO imulup: authenticated as GitHub App your-app[bot]
INFO imulup: running as user imulup
INFO imulup: running checkouts and steps as user imulup-runner
INFO imulup: imulup listening on 0.0.0.0:4141
```

## 5. Open a pull request

On a new branch, change `quickstart/Pulumi.yaml` so the pet name has three words:

```yaml
resources:
  pet:
    type: random:RandomPet
    properties:
      length: 3
```

Open a pull request. Within a few seconds, imulup:

1. sets a pending `imulup/plan` commit status,
2. runs the plan workflow, and
3. comments with the preview diff, headed **imulup plan — 1/1 project succeeded**.

Now comment on the PR:

```text
imulup apply
```

imulup reacts with 👀, runs `pulumi up`, and comments with the output. The `imulup/apply` status turns green.

Merge or close the PR, and imulup releases the `quickstart:dev` lock and deletes its checkout.

## Clean up

- Stop the container and delete its volume: `docker volume rm imulup-quickstart`.
- Uninstall or delete the GitHub App, and delete `app.pem`.

## Next steps

- [Deploy with Docker](/imulup/install/docker/) for a long-running setup.
- [Configure projects and stacks](/imulup/usage/projects/) for a real repository.
- Read the [security model](/imulup/security/model/) before you give imulup real cloud credentials.
