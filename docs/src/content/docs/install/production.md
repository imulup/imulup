---
title: Running in production
description: Reverse proxies, which endpoints must be public, where state lives, and how to keep it.
---

## Expose only the webhook

GitHub only needs to reach `POST /events`. Webhook requests are authenticated by their HMAC signature, so that
endpoint can be public. The other endpoints have no authentication. Keep them private, or put them behind your
proxy's auth:

| Endpoint | Public? | Purpose |
| --- | --- | --- |
| `POST /events` | Yes | GitHub webhooks |
| `GET /healthz` | No | Health check for your load balancer or orchestrator |
| `GET /` | No | HTML table of the held locks |
| `GET /api/locks` | No | The held locks as JSON |

The lock pages show repository names, stack names and PR authors. See the [HTTP reference](/imulup/reference/http/).

## Behind nginx

Bind imulup to loopback. With Compose, set `IMULUP_PUBLISH_PORT=127.0.0.1:4141`; with the binary, set
`IMULUP_BIND=127.0.0.1`. Then terminate TLS in nginx:

```nginx
server {
    listen 443 ssl;
    server_name imulup.example.com;
    # ssl_certificate / ssl_certificate_key …

    # GitHub webhooks: public, POST only.
    location = /events {
        limit_except POST { deny all; }
        client_max_body_size 25m;
        proxy_pass http://127.0.0.1:4141;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    # Everything else: lock view and health check, behind basic auth.
    location / {
        auth_basic "imulup";
        auth_basic_user_file /etc/nginx/imulup.htpasswd;
        proxy_pass http://127.0.0.1:4141;
        proxy_set_header Host $host;
    }
}
```

GitHub caps webhook payloads at 25 MB, which is what `client_max_body_size` allows. imulup only reads a few fields from
each payload.

Webhook deliveries return quickly. imulup answers `202 Accepted` and does the plan or apply in the background, so
you don't need long proxy timeouts.

## State and the data directory

Everything imulup keeps is under `IMULUP_DATA_DIR` (`/data` in the images):

| Path | Contents |
| --- | --- |
| `state.json` | Locks and plan records. Written atomically on every change. |
| `mirrors/<owner>/<repo>.git` | Bare mirrors of each repository, fetched with credentials. |
| `repos/<owner>/<repo>/<pr>` | One working tree per open PR. Deleted when the PR closes. |

Only `state.json` matters. The mirrors and checkouts are caches: imulup recreates them on the next fetch. If you lose
`state.json`, every lock and plan is dropped, and open PRs just need a fresh `imulup plan`.

Pulumi's own state isn't here. It lives in your [Pulumi backend](/imulup/pulumi/backends/).

To back up, copy `state.json` while imulup is idle, or snapshot the volume.

## One instance only

The state is a single file with no coordination between processes, so run exactly one imulup server.

- On Kubernetes, use `replicas: 1` with the `Recreate` deployment strategy, so the old and new pods never overlap.
- Give it a persistent volume for the data directory.

imulup doesn't ship a Helm chart yet.

If imulup restarts during a plan or apply, that run doesn't finish and doesn't get a PR comment. Locks and
recorded plans survive the restart, so comment `imulup plan` or `imulup apply` again.

## Logging

imulup logs to stderr. `RUST_LOG` sets the level, and the default is `info`:

```sh
RUST_LOG=info                     # one line per accepted webhook, plus errors
RUST_LOG=imulup=debug             # also why each ignored webhook was ignored
```

Step output isn't logged. It goes to the PR comment, with GitHub tokens and the webhook secret redacted.

## Health checks

`GET /healthz` returns `200 ok` once the server is listening. It doesn't check GitHub or Pulumi connectivity.

## Resource sizing

Each PR gets its own checkout, and the language runtimes' dependency caches persist between runs. Allow disk for
your repositories times the number of open PRs, plus package caches.

imulup runs one job at a time for each PR. Jobs on different PRs run in parallel, and each runs Pulumi on the host,
so size CPU and memory for your busiest burst of pushes.
