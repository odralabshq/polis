# syntax=docker/dockerfile:1
FROM dhi.io/rust:1-dev@sha256:8cfde71b7c05f77d045f83f1fcb806aac095a447e84b6c705d3f2447c05c9aa4 AS builder
WORKDIR /build
COPY services/control-plane/crates/ crates/
COPY services/control-plane/Cargo.toml services/control-plane/Cargo.lock* ./
COPY lib/crates/ ../../lib/crates/
COPY services/sentinel/modules/dlp/srv_polis_dlp.c /sentinel/modules/dlp/srv_polis_dlp.c
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    cargo build --release -p cp-server && \
    cp target/release/cp-server /cp-server

FROM dhi.io/debian-base:trixie@sha256:9ef766670af4743904b0f992a26b525c6c914648b56ea597ec23452adcf1a95d

COPY --from=builder /cp-server /usr/local/bin/cp-server

USER 65532

ENV RUST_LOG=info
ENV POLIS_CP_LISTEN_ADDR=0.0.0.0:9080
ENV POLIS_CP_VALKEY_URL=rediss://valkey:6379

EXPOSE 9080
CMD ["/usr/local/bin/cp-server"]
