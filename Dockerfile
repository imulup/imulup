# syntax=docker/dockerfile:1
#
# One Dockerfile for every imulup image. PULUMI_IMAGE picks the runtime base:
#   pulumi/pulumi          all language runtimes ("full" variant)
#   pulumi/pulumi-<lang>   nodejs | python | go | dotnet | java
# Use docker-bake.hcl to build all variants with consistent tags.
ARG PULUMI_VERSION=3.277.0
ARG PULUMI_IMAGE=pulumi/pulumi

FROM rust:1-bookworm AS build
WORKDIR /src
# Build dependencies first so source-only changes reuse this layer.
COPY Cargo.toml Cargo.lock ./
# Cargo.toml declares a test target, so its file must exist for the manifest to parse.
RUN mkdir src tests && echo 'fn main() {}' > src/main.rs && touch src/lib.rs \
    && echo 'fn main() {}' > tests/privsep.rs \
    && cargo build --release --locked \
    && rm -rf src
COPY src ./src
RUN touch src/main.rs src/lib.rs && cargo build --release --locked

FROM ${PULUMI_IMAGE}:${PULUMI_VERSION}
ARG PULUMI_IMAGE
ARG PULUMI_VERSION
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*
# imulup starts as root to read its secrets, then runs as `imulup`. PR code (checkouts and
# steps) runs as `imulup-runner`, which can't read imulup's secrets, state, or process. The
# fixed, high ids avoid matching the owner of files bind-mounted from the host.
RUN groupadd --system --gid 10001 imulup \
    && useradd --system --uid 10001 --gid imulup --home-dir /nonexistent --no-create-home \
       --shell /usr/sbin/nologin imulup \
    && groupadd --gid 10002 imulup-runner \
    && useradd --uid 10002 --gid imulup-runner --create-home --home-dir /home/imulup-runner \
       --shell /bin/sh imulup-runner \
    # Some base images set toolchains up for root: .NET under /root/.dotnet (and /root is 0700),
    # and a GOPATH that is missing or root-owned. Let the runner use them.
    && chmod 0711 /root \
    && if command -v go >/dev/null; then \
         mkdir -p "${GOPATH:-/go}" && chown imulup-runner:imulup-runner "${GOPATH:-/go}"; \
       fi
COPY --from=build /src/target/release/imulup /usr/local/bin/imulup
LABEL org.opencontainers.image.title="imulup" \
      org.opencontainers.image.description="Atlantis-style pull request automation for Pulumi" \
      org.opencontainers.image.base.name="${PULUMI_IMAGE}:${PULUMI_VERSION}"
ENV IMULUP_DATA_DIR=/data \
    IMULUP_SERVER_USER=imulup \
    IMULUP_STEP_USER=imulup-runner
VOLUME /data
EXPOSE 4141
ENTRYPOINT ["/usr/local/bin/imulup"]
