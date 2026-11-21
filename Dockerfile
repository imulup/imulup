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
RUN mkdir src && echo 'fn main() {}' > src/main.rs && touch src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src
COPY src ./src
RUN touch src/main.rs src/lib.rs && cargo build --release --locked

FROM ${PULUMI_IMAGE}:${PULUMI_VERSION}
ARG PULUMI_IMAGE
ARG PULUMI_VERSION
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/imulup /usr/local/bin/imulup
LABEL org.opencontainers.image.title="imulup" \
      org.opencontainers.image.description="Atlantis-style pull request automation for Pulumi" \
      org.opencontainers.image.base.name="${PULUMI_IMAGE}:${PULUMI_VERSION}"
ENV IMULUP_DATA_DIR=/data
VOLUME /data
EXPOSE 4141
ENTRYPOINT ["/usr/local/bin/imulup"]
