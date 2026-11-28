---
title: State backends
description: Use Pulumi Cloud or a self-managed backend (S3, GCS, Azure Blob) with imulup.
---

imulup doesn't store Pulumi state. It runs the Pulumi CLI, and the CLI uses whichever backend its environment points
to. Set the backend in imulup's environment: every step inherits it.

## Pulumi Cloud

```sh
PULUMI_ACCESS_TOKEN=pul-...
```

Use an [organization or team access token](https://www.pulumi.com/docs/pulumi-cloud/access-management/access-tokens/)
rather than a personal one, so imulup's access doesn't depend on one person's account.

Pulumi Cloud is also required for [ESC environments](/pulumi/credentials/#pulumi-esc).

## Self-managed backends

Set `PULUMI_BACKEND_URL`, plus a way to decrypt stack secrets:

```sh
# Amazon S3
PULUMI_BACKEND_URL=s3://my-pulumi-state?region=us-east-1
# Google Cloud Storage
PULUMI_BACKEND_URL=gs://my-pulumi-state
# Azure Blob Storage
PULUMI_BACKEND_URL=azblob://my-pulumi-state
AZURE_STORAGE_ACCOUNT=mystorageaccount

# Stacks that use the passphrase secrets provider
PULUMI_CONFIG_PASSPHRASE=...
```

The backend also needs credentials, for example `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` or an instance role for
S3. See [Cloud credentials](/pulumi/credentials/).

The bucket must already exist. Pulumi doesn't create it, and a missing bucket fails the plan with a `NoSuchBucket`
error. For S3-compatible storage such as MinIO or Cloudflare R2, add the `endpoint` (and usually `region`) query
parameters. Pulumi's [state and backends](https://www.pulumi.com/docs/iac/concepts/state-and-backends/) page lists
all the options.

Stacks that use a cloud KMS secrets provider (`awskms://`, `gcpkms://`, `azurekeyvault://`, `hashivault://`) record it
in `Pulumi.<stack>.yaml`. They only need credentials that can use the key.

### A different backend per project

A project can name its own backend in `Pulumi.yaml`. If your projects do this, leave `PULUMI_BACKEND_URL` unset
so the two don't compete:

```yaml
name: network
runtime: nodejs
backend:
  url: s3://network-state?region=eu-west-1
```

### Local file backend

`file://` backends work, but the state then lives on the imulup host and needs its own backups. In the Docker images,
steps run as `imulup-runner` and can't write to `/data`. A path such as `file://~` puts the state in the runner's home
directory, which isn't on a volume unless you add one. That's fine for the [quickstart](/quickstart/), but use
object storage for anything real.
