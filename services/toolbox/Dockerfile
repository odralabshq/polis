FROM rust:1.85-bookworm AS builder
WORKDIR /build
COPY services/toolbox/crates/ services/toolbox/crates/
COPY lib/crates/ lib/crates/
COPY Cargo.toml Cargo.lock ./

# Install nightly toolchain (darling 0.23 requires rustc 1.88+)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup default nightly-2026-02-01

RUN cargo build --release -p polis-mcp-agent

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && echo 'toolbox:x:65532:65532:toolbox:/home/toolbox:/sbin/nologin' >> /etc/passwd \
    && echo 'toolbox:x:65532:' >> /etc/group

COPY --from=builder /build/target/release/polis-mcp-agent /usr/bin/

USER toolbox

ENV RUST_LOG=info
ENV polis_AGENT_LISTEN_ADDR=0.0.0.0:8080
ENV polis_AGENT_VALKEY_URL=redis://valkey:6379
EXPOSE 8080
CMD ["/usr/bin/polis-mcp-agent"]
