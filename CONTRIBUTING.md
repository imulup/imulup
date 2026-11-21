# Contributing to imulup

Thanks for your interest in improving imulup! Bug reports, docs fixes, and code are all welcome.

By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md). Report security problems
privately as described in [SECURITY.md](SECURITY.md), not in public issues.

## Getting started

You need a stable Rust toolchain (1.88 or newer) and `git`. You don't need Pulumi or a GitHub account for the
tests: the end-to-end tests use a fake GitHub API, a local git remote, and a stub `pulumi` script.

```sh
git clone <your fork>
cd imulup
cargo test
```

## Before you open a PR

CI runs these checks, and every one must pass:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo deny check          # needs cargo-deny: https://github.com/EmbarkStudios/cargo-deny
```

The lint configuration is in `Cargo.toml` under `[lints]`: it enables `clippy::all` and `clippy::pedantic`, and
forbids `unsafe` code. Fix lints rather than allowing them. If an `#[allow]` is genuinely right, add a comment
explaining why.

To check the container images:

```sh
docker buildx bake --load nodejs     # or omit the target to build every variant
```

## Pull requests

- Keep PRs focused on one change. Open an issue first for anything large or that changes behavior, so we can
  agree on the approach.
- Add or update tests. Behavior changes to webhook handling, locking, or plan/apply should have coverage in
  `tests/e2e.rs`.
- Update `README.md` when you add or change flags, `imulup.yaml` fields, or comment commands.
- New dependencies must pass `cargo deny check`: permissive licenses only, and no unmaintained crates.

## Releases

Members of the `release-managers` team create a release by pushing a semver tag:

```sh
git tag v0.2.0 && git push origin v0.2.0
```

A tag ruleset rejects `v*` tags pushed by anyone outside that team. The release workflow runs in the protected
`release` environment, so it waits for a `release-managers` approval before it builds and publishes every container
image variant.

## License

By contributing, you agree that your contributions are licensed under the [Apache License 2.0](LICENSE).
