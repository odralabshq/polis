# syntax=docker/dockerfile:1
FROM rust:1.88-slim AS builder
WORKDIR /build
COPY services/toolbox/crates/ services/toolbox/crates/
COPY lib/crates/ lib/crates/
COPY Cargo.toml Cargo.lock ./
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    cargo build --release -p polis-toolbox && \
    cp target/release/polis-toolbox /polis-toolbox

FROM dhi.io/debian-base:trixie

# DHI debian-base includes ca-certificates; no apt-get/useradd in minimal image

COPY --from=builder /polis-toolbox /usr/bin/

# DHI images run as nonroot (UID 65532) by default
# Valkey TLS certs are mounted :ro at runtime
USER nonroot

ENV RUST_LOG=info
ENV polis_AGENT_LISTEN_ADDR=0.0.0.0:8080
ENV polis_AGENT_VALKEY_URL=redis://valkey:6379
EXPOSE 8080
CMD ["/usr/bin/polis-toolbox"]
