# imulup

Atlantis-style pull request automation for [Pulumi](https://www.pulumi.com/), written in Rust.

imulup listens for GitHub webhooks. When a PR changes a Pulumi project it runs `pulumi preview` and
posts the diff as a PR comment. Reviewers then comment `imulup apply` to run `pulumi up`. While a PR
has a plan for a stack, that stack is locked so no other PR can plan or apply it in the meantime.

## How it works

| Atlantis | imulup |
| --- | --- |
| `atlantis plan` → `terraform plan` | `imulup plan` → `pulumi preview --diff` |
| `atlantis apply` → `terraform apply` | `imulup apply` → `pulumi up --yes --skip-preview` |
| dir + workspace | dir + **stack** |
| `atlantis.yaml` | `imulup.yaml` |

* **Autoplan.** When a PR is opened or pushed to, imulup plans every affected project. Draft PRs are skipped unless `--plan-drafts` is set.
* **Locks.** Each lock covers one `(repo, dir, stack)` and belongs to the PR that planned it. imulup releases the lock when that PR is merged or closed, or when someone comments `imulup unlock`.
* **Stale-plan protection.** `apply` only runs when there is a successful plan for the PR's current head commit. Any new push means you must plan again.
* **Apply requirements.** You can require `approved` and/or `mergeable` per project, or server-wide.
* **Commit statuses.** imulup sets `imulup/plan` and `imulup/apply` statuses. Mark them as required checks to block merging until changes are applied.
* **Lock view.** `GET /` shows an HTML table of the locks. `GET /api/locks` returns them as JSON.

## PR comment commands

```
imulup plan   [-p project | -d dir [-s stack]]
imulup apply  [-p project | -d dir [-s stack]]
imulup unlock
imulup help
```

With no flags, `plan` plans every project the PR modifies, and `apply` applies every project with a
successful plan that hasn't been applied yet.

By default, only users with write access to the repo can run commands. Turn this off with `--skip-collaborator-check`.

## Project discovery

**Without `imulup.yaml`:** for each modified file, imulup walks up the directory tree to the nearest
`Pulumi.yaml`. It then plans every stack that has a `Pulumi.<stack>.yaml` file in that directory.

**With `imulup.yaml`** at the repo root, you list projects explicitly:

```yaml
version: 1
projects:
  - name: network-prod          # optional; enables `-p network-prod`
    dir: network
    stack: prod
    apply_requirements: [approved, mergeable]
    autoplan:
      enabled: true             # default true
      when_modified: ["**/*.ts", "Pulumi*.yaml", "../shared/**"]  # relative to dir; default ["**/*"]
    workflow: node
  - dir: network
    stack: dev

workflows:
  node:
    plan:
      steps:
        - run: npm ci           # runs via `sh -c` in the project dir
        - preview:
            extra_args: ["--refresh"]
    apply:
      steps:
        - run: npm ci
        - up
```

Steps can be `preview`, `up`, `preview: {extra_args: [...]}`, `up: {extra_args: [...]}`, or
`run: <shell>`. `run` steps get `IMULUP_STACK`, `IMULUP_PROJECT`, `IMULUP_DIR`, `IMULUP_REPO`,
`IMULUP_PULL`, `IMULUP_HEAD_SHA`, and `IMULUP_USER` as environment variables.

The default workflow is just `preview` / `up`. That means language dependencies (`npm ci`, `pip install`, …) must either be installed
by a `run` step or already present. Checkouts are reused between runs, so dependencies under ignored paths such as
`node_modules` persist.

## Running

```sh
export IMULUP_GH_TOKEN=ghp_...              # repo scope: contents, pull requests, statuses
export IMULUP_GH_WEBHOOK_SECRET=...         # same secret as on the webhook
export IMULUP_GH_USER=my-imulup-bot         # login that owns the token
export IMULUP_REPO_ALLOWLIST='my-org/*'
export PULUMI_ACCESS_TOKEN=...              # plus cloud credentials your programs need
cargo run --release
```

Then configure a GitHub webhook:

* **Payload URL:** `https://<host>/events`
* **Content type:** `application/json`
* **Secret:** the same value as `IMULUP_GH_WEBHOOK_SECRET`
* **Events:** *Pull requests* and *Issue comments*

Run `imulup --help` to see every flag. Each flag also has an `IMULUP_*` environment variable.

### Docker

Images come in a "full" variant with every Pulumi language runtime, and a slim variant per language:

| Tag | Base | Size (arm64) |
| --- | --- | --- |
| `imulup:<version>`, `imulup:latest` | `pulumi/pulumi` (all languages) | ~14 GB |
| `imulup:<version>-nodejs`, `imulup:nodejs` | `pulumi/pulumi-nodejs` | ~1.2 GB |
| `imulup:<version>-python`, `imulup:python` | `pulumi/pulumi-python` | ~0.8 GB |
| `imulup:<version>-go`, `imulup:go` | `pulumi/pulumi-go` | ~0.8 GB |
| `imulup:<version>-dotnet`, `imulup:dotnet` | `pulumi/pulumi-dotnet` | ~1.4 GB |
| `imulup:<version>-java`, `imulup:java` | `pulumi/pulumi-java` | ~1.8 GB |

Every variant uses the same Pulumi version, which `PULUMI_VERSION` in `docker-bake.hcl` pins. Build them with
[Bake](https://docs.docker.com/build/bake/):

```sh
docker buildx bake --load                # all variants for the local platform
docker buildx bake --load nodejs         # just one
VERSION=0.2.0 REGISTRY=ghcr.io/acme docker buildx bake --push \
  --set '*.platform=linux/amd64,linux/arm64'
```

Pushing a `v*` git tag runs `.github/workflows/release.yml`, which publishes every variant for amd64 and arm64 to
`ghcr.io/<owner>/imulup`.

```sh
docker run -p 4141:4141 -e IMULUP_GH_TOKEN -e IMULUP_GH_WEBHOOK_SECRET -e IMULUP_GH_USER \
  -e IMULUP_REPO_ALLOWLIST -e PULUMI_ACCESS_TOKEN -v imulup-data:/data imulup:nodejs
```

## Security model

Unlike Terraform HCL, **a Pulumi program is arbitrary code**, and so is a `run:` step. Anyone who can open a PR
against an allowlisted repo can run code on the imulup host with whatever cloud credentials you give it. To limit this:

* PRs from forks are ignored by default (`--allow-fork-prs` turns them back on).
* Commands require write access to the repo.
* Webhooks without a valid `X-Hub-Signature-256` signature are rejected.
* All `IMULUP_*` variables (including the GitHub token and the webhook secret) are removed from the environment before steps run. The token is also redacted from any output posted to the PR.
* git credentials are passed through environment-based config, so they never appear in `.git/config` or the process args.

Give the credentials only the scope they need, and limit `--repo-allowlist` to repos whose contributors you trust.

## Limitations / roadmap

* GitHub only, authenticated with a token. GitHub App auth and GitLab aren't supported yet; `vcs::Vcs` is the extension point.
* `apply` re-runs `pulumi up` against the same commit. It doesn't use Pulumi's experimental `--save-plan` / `--plan`.
* State is a single JSON file (`<data-dir>/state.json`), so you can only run one imulup instance.
* `mergeable` checks only for merge conflicts. It doesn't evaluate branch protection rules.

## Development

```sh
cargo test                                   # unit + end-to-end tests (fake GitHub, real git, fake pulumi)
cargo clippy --all-targets -- -D warnings    # lint (must be clean)
cargo fmt --check
```

Linting is done with Clippy. The lint configuration is in `Cargo.toml` under `[lints]`: it enables `clippy::all` and `clippy::pedantic`, warns on `unwrap_used`, `dbg_macro`, and `todo`, and denies `unsafe_code`. `clippy.toml` allows `unwrap`/`expect` inside tests. CI (`.github/workflows/ci.yml`) runs fmt, clippy with `-D warnings`, the tests, a build at the minimum supported
Rust version (1.88), and `cargo deny` (advisories and licenses) on every push and PR.

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for the workflow and the
[Code of Conduct](CODE_OF_CONDUCT.md). Report security issues privately, following [SECURITY.md](SECURITY.md).

## License

imulup is licensed under the [Apache License 2.0](LICENSE).
