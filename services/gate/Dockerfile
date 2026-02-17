# syntax=docker/dockerfile:1
# Polis Gateway with g3proxy + g3fcgen
# Compiled from source v1.12.2

# ===========================================
# Stage 1: Build from source
# ===========================================
FROM dhi.io/rust:1-dev@sha256:8cfde71b7c05f77d045f83f1fcb806aac095a447e84b6c705d3f2447c05c9aa4 AS builder

# Install build dependencies (gzip needed for tar extraction)
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    libssl-dev \
    cmake \
    capnproto \
    libcapnp-dev \
    liblua5.4-dev \
    libc-ares-dev \
    libcap-dev \
    python3-dev \
    libunwind-dev \
    libnuma-dev \
    curl \
    gzip \
    && rm -rf /var/lib/apt/lists/*

# Install Rust nightly pinned to specific date (g3 requires rustc 1.90+ and nightly features)
# DHI rust:1-dev has stable only; install rustup for nightly
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup default nightly-2026-02-01 && rustup component add rust-src

# Download and verify g3 source v1.12.2 (supply chain security)
WORKDIR /build
ENV G3_SHA256=4aff3f3ea50774b5346859b3ef1f120c5dba70e6cef168fbb9ccdc9168fa0ff5
RUN curl -L https://github.com/bytedance/g3/archive/refs/tags/g3proxy-v1.12.2.tar.gz -o g3.tar.gz && \
    echo "${G3_SHA256}  g3.tar.gz" | sha256sum -c - && \
    tar -xzf g3.tar.gz && \
    mv g3-g3proxy-v1.12.2 g3

# Build g3proxy and g3fcgen with release-lto profile
WORKDIR /build/g3
RUN --mount=type=cache,target=/root/.cargo/registry,sharing=locked \
    --mount=type=cache,target=/root/.cargo/git,sharing=locked \
    --mount=type=cache,target=/build/g3/target,sharing=locked \
    cargo build --release -p g3proxy -p g3fcgen && \
    cp target/release/g3proxy /g3proxy && \
    cp target/release/g3fcgen /g3fcgen

# ===========================================
# Stage 2: Runtime image (needs apt for runtime deps)
# ===========================================
FROM dhi.io/debian-base:trixie-dev@sha256:2166e2eaef0651c9ad21de6ab5a34fda12541d89bccf7bcb0a94afceb1b1541b

# Install runtime dependencies (util-linux provides setpriv for privilege dropping with ambient caps)
# Note: apt-get may overwrite /etc/passwd, so we ensure nonroot user exists after
RUN apt-get update && apt-get install -y --no-install-recommends \
    iproute2 \
    nftables \
    openssl \
    ca-certificates \
    curl \
    libssl3t64 \
    liblua5.4-0 \
    libc-ares2 \
    libcap2 \
    python3 \
    libpython3.13 \
    libunwind8 \
    libnuma1 \
    procps \
    netcat-openbsd \
    util-linux \
    && rm -rf /var/lib/apt/lists/* \
    && (grep -q '^nonroot:' /etc/passwd || echo 'nonroot:x:65532:65532:nonroot:/home/nonroot:/sbin/nologin' >> /etc/passwd) \
    && (grep -q '^nonroot:' /etc/group || echo 'nonroot:x:65532:' >> /etc/group)

# Copy binaries from builder stage
COPY --from=builder /g3proxy /usr/bin/g3proxy
COPY --from=builder /g3fcgen /usr/bin/g3fcgen

# Create directories with correct ownership for DHI nonroot user (65532)
RUN mkdir -p /etc/g3proxy /var/lib/g3proxy/ssl /tmp/g3 && \
    mkdir -p /var/log/g3proxy && \
    chown -R 65532:65532 /var/log/g3proxy /var/lib/g3proxy /tmp/g3

# Verify binaries
# Note: setcap is NOT used because no-new-privileges:true in compose prevents
# execve from honoring file capabilities. Capabilities are granted via setpriv
# ambient caps in init.sh instead.
RUN g3proxy --version && g3fcgen --version

# Copy scripts
COPY services/gate/scripts/init.sh /init.sh
COPY services/gate/scripts/setup-network.sh /setup-network.sh
RUN chmod +x /init.sh /setup-network.sh

USER nonroot
ENTRYPOINT ["/init.sh"]
