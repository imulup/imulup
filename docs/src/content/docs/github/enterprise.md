---
title: GitHub Enterprise Server
description: Point imulup at a GitHub Enterprise Server instance.
---

imulup works with GitHub Enterprise Server. Set the API base URL of your instance:

```sh
IMULUP_GH_API_URL=https://ghe.example.com/api/v3
```

Everything else is the same as on github.com. Create a [GitHub App](/github/app/) on your instance, or use a
[token](/github/token/) and webhook. imulup fetches PR heads from the clone URL that the API returns, so git
traffic goes to your instance too.

One known gap: the PR links on the [lock view](/reference/http/) (`GET /`) always point at `github.com`.
