# syntax=docker/dockerfile:1
# Shared g3 builder - builds g3proxy and g3fcgen binaries
# Used by: gate, certgen

FROM dhi.io/rust:1-dev@sha256:8cfde71b7c05f77d045f83f1fcb806aac095a447e84b6c705d3f2447c05c9aa4

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential pkg-config libssl-dev cmake capnproto libcapnp-dev \
    liblua5.4-dev libc-ares-dev libcap-dev python3-dev libunwind-dev \
    libnuma-dev curl gzip && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup default nightly-2026-02-01 && rustup component add rust-src

WORKDIR /build
ENV G3_SHA256=4aff3f3ea50774b5346859b3ef1f120c5dba70e6cef168fbb9ccdc9168fa0ff5
RUN curl -L https://github.com/bytedance/g3/archive/refs/tags/g3proxy-v1.12.2.tar.gz -o g3.tar.gz && \
    echo "${G3_SHA256}  g3.tar.gz" | sha256sum -c - && \
    tar -xzf g3.tar.gz && mv g3-g3proxy-v1.12.2 g3

WORKDIR /build/g3
RUN --mount=type=cache,target=/root/.cargo/registry,sharing=locked \
    --mount=type=cache,target=/root/.cargo/git,sharing=locked \
    --mount=type=cache,target=/build/g3/target,sharing=locked \
    cargo build --release -p g3proxy -p g3fcgen && \
    cp target/release/g3proxy /g3proxy && \
    cp target/release/g3fcgen /g3fcgen
