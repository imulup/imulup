# Security policy

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

Report vulnerabilities privately through GitHub's
[private vulnerability reporting](../../security/advisories/new). You can also find it under the repository's
**Security** tab: choose **Report a vulnerability**.

Please include:

- the affected version or commit, and the image variant if relevant
- a description of the issue and its impact
- steps to reproduce, or a proof of concept
- any suggested fix or mitigation

We'll acknowledge the report as soon as we can and keep you updated while we fix it. Once a fix is released, we
publish a GitHub security advisory and credit you unless you'd rather stay anonymous.

## Supported versions

Security fixes go into the latest release. Please upgrade before reporting, if you can.

## Scope

imulup runs Pulumi programs and `run:` steps from pull requests, and **these are arbitrary code by design**.
It's expected that someone allowed to open PRs against an allowlisted repo can execute code on the imulup host.
See [Security model](README.md#security-model) for the controls that limit this.

Issues in scope include, for example:

- bypassing webhook signature verification, the repo allowlist, the fork-PR restriction, or the write-access check
- applying changes without a current, successful plan, or without meeting the apply requirements
- leaking the GitHub token, an installation token, the GitHub App key, or the webhook secret (into PR comments,
  logs, or the environment of user code)
- escaping the per-PR workspace with paths from `imulup.yaml` or PR content
- denial of service against the server from unauthenticated webhook requests
