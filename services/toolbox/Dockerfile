# syntax=docker/dockerfile:1
FROM dhi.io/rust:1-dev@sha256:8cfde71b7c05f77d045f83f1fcb806aac095a447e84b6c705d3f2447c05c9aa4 AS builder
WORKDIR /build
COPY services/toolbox/crates/ crates/
COPY services/toolbox/Cargo.toml services/toolbox/Cargo.lock* ./
COPY lib/crates/ ../../lib/crates/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    cargo build --release -p polis-hitl-agent && \
    cp target/release/polis-hitl-agent /polis-mcp-agent

FROM dhi.io/debian-base:trixie@sha256:9ef766670af4743904b0f992a26b525c6c914648b56ea597ec23452adcf1a95d

# DHI debian-base includes ca-certificates; no apt-get/useradd in minimal image

COPY --from=builder /polis-mcp-agent /usr/local/bin/polis-mcp-agent

# DHI images run as nonroot (UID 65532) by default
# Valkey TLS certs are mounted :ro at runtime
USER 65532

ENV RUST_LOG=info
ENV polis_AGENT_LISTEN_ADDR=0.0.0.0:8080
ENV polis_AGENT_VALKEY_URL=redis://valkey:6379
EXPOSE 8080
CMD ["/usr/local/bin/polis-mcp-agent"]
