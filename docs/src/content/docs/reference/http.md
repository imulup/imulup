---
title: HTTP endpoints
description: The endpoints the imulup server exposes.
---

imulup serves HTTP on `--bind`:`--port` (`0.0.0.0:4141` by default). It doesn't terminate TLS: put a reverse proxy in
front of it. See [Running in production](/install/production/).

## `POST /events`

The GitHub webhook endpoint. It is the only endpoint that needs to be public.

| Response | When |
| --- | --- |
| `202 accepted` | The event is handled in the background |
| `200 pong` | A `ping` event |
| `200 <reason>` | The event was ignored, for example `pull_request action not handled` or `comment is not on a pull request` |
| `400 invalid JSON` | The body isn't JSON. Check that the webhook's content type is `application/json`. |
| `401 bad signature` | `X-Hub-Signature-256` is missing or doesn't match the webhook secret |
| `403 repo not allowlisted` | The repository isn't on `--repo-allowlist` |

Handled events:

| `X-GitHub-Event` | Actions |
| --- | --- |
| `pull_request` | `opened`, `reopened`, `synchronize`, `ready_for_review` (autoplan); `closed` (release locks) |
| `issue_comment` | `created`, on pull requests only |

## `GET /healthz`

Returns `200 ok` while the server is running.

## `GET /`

An HTML page listing the held [locks](/usage/locking/), with links to the PRs that hold them.

## `GET /api/locks`

The same locks as a JSON array:

```json
[
  {
    "repo": "my-org/infra",
    "dir": "network",
    "stack": "prod",
    "project": "network-prod",
    "pull": 42,
    "user": "alice",
    "created_at": 1764201600
  }
]
```

`/`, `/api/locks` and `/healthz` have no authentication. Keep them off the public internet, or put them behind your
proxy's auth.

## Metrics

imulup doesn't expose metrics yet. Use the logs (`RUST_LOG=info` logs one line per accepted event) and the
commit statuses to monitor it.
