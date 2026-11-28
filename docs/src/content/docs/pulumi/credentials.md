---
title: Cloud credentials
description: Give Pulumi programs the cloud credentials they need, with environment variables, instance roles, or Pulumi ESC.
---

Pulumi programs need credentials for the clouds and services they manage. imulup passes them through from its own
environment.

## What steps can see

Every step gets imulup's environment, with two changes:

- All `IMULUP_*` variables are removed, including the GitHub token, the app key and the webhook secret. imulup then
  sets its own `IMULUP_STACK`, `IMULUP_PROJECT` and so on (see the
  [step environment](/reference/step-environment/)).
- With privilege separation, `HOME`, `USER` and `LOGNAME` are set to the step user's.

Everything else passes through unchanged: `PULUMI_ACCESS_TOKEN`, `AWS_*`, `GOOGLE_*`, `ARM_*` and so on.

:::danger
Whatever a step can use, a pull request can use too. A PR can change the Pulumi program, or add a `run:` step, to
print or send off any credential in the environment. Only allowlist repositories whose contributors you'd trust with
those credentials. See the [security model](/security/model/).
:::

## Environment variables

The simplest setup is to put credentials in imulup's environment, for example in the Compose `.env` file:

```sh
AWS_ACCESS_KEY_ID=...
AWS_SECRET_ACCESS_KEY=...
AWS_REGION=us-east-1
```

Files, such as a GCP service-account key in `GOOGLE_APPLICATION_CREDENTIALS`, must be readable by the step user
(`imulup-runner`, uid 10002, in the images). The key file for imulup's own GitHub App doesn't need to be readable by it,
and shouldn't be.

## Instance and workload identity

Credentials from the platform work without any configuration in imulup:

- AWS: an EC2 instance profile, ECS task role, or EKS IRSA / Pod Identity
- Google Cloud: the VM's or GKE workload's service account
- Azure: a managed identity

The cloud SDKs inside Pulumi providers find them on their own. These are short-lived, and nothing has to be stored.

## Pulumi ESC

With Pulumi Cloud, a stack can import an [ESC environment](https://www.pulumi.com/docs/esc/) in its config file.
ESC can issue short-lived cloud credentials through OIDC, and different stacks can use different environments:

```yaml title="Pulumi.prod.yaml"
environment:
  - aws-prod
```

```yaml title="Pulumi.dev.yaml"
environment:
  - aws-dev
```

`pulumi preview` and `pulumi up` open the environment themselves, so imulup only needs `PULUMI_ACCESS_TOKEN`. That
token has to be allowed to open the environments.

## Different credentials per stack

imulup gives every step the same environment, for every repo, project and stack. To give `prod` stronger credentials
than `dev`:

- Use per-stack ESC environments, as shown above.
- Or assume a role in the program or the provider configuration, for example the AWS provider's `assumeRole` set in
  `Pulumi.prod.yaml`.

Either way, any PR that can plan the `prod` stack can reach the `prod` credentials. Apply requirements only gate
`imulup apply`, and a plan runs the program too. Rely on the repo allowlist and on who can open PRs, rather than on
per-stack settings, to protect production credentials.
