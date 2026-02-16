# Local build of dhi.io/rust:1-dev
# Rust stable + nightly toolchain with nonroot user (UID 65532)
FROM rust:1.85-bookworm

RUN groupadd -g 65532 nonroot \
    && useradd -u 65532 -g 65532 -d /home/nonroot -s /sbin/nologin -M nonroot
