# Polis Gateway with g3proxy + g3fcgen
# Compiled from source v1.12.2

# ===========================================
# Stage 1: Build from source
# ===========================================
FROM rust:1-trixie AS builder

# Install build dependencies
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
    && rm -rf /var/lib/apt/lists/*

# Install Rust nightly (g3 requires rustc 1.90+)
RUN rustup default nightly && rustup update nightly

# Download and verify g3 source v1.12.2 (supply chain security)
WORKDIR /build
ENV G3_SHA256=4aff3f3ea50774b5346859b3ef1f120c5dba70e6cef168fbb9ccdc9168fa0ff5
RUN curl -L https://github.com/bytedance/g3/archive/refs/tags/g3proxy-v1.12.2.tar.gz -o g3.tar.gz && \
    echo "${G3_SHA256}  g3.tar.gz" | sha256sum -c - && \
    tar -xzf g3.tar.gz && \
    mv g3-g3proxy-v1.12.2 g3

# Build g3proxy and g3fcgen with release-lto profile
WORKDIR /build/g3
RUN cargo build --release -p g3proxy -p g3fcgen

# ===========================================
# Stage 2: Runtime image
# ===========================================
FROM debian:trixie

# Install runtime dependencies (util-linux provides setpriv for privilege dropping with ambient caps)
RUN apt-get update && apt-get install -y --no-install-recommends \
    iproute2 \
    iptables \
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
    && rm -rf /var/lib/apt/lists/*

# Copy binaries from builder stage
COPY --from=builder /build/g3/target/release/g3proxy /usr/bin/g3proxy
COPY --from=builder /build/g3/target/release/g3fcgen /usr/bin/g3fcgen

# Create dedicated non-root user for g3proxy
RUN groupadd -r g3proxy && useradd -r -g g3proxy -s /sbin/nologin g3proxy

# Create directories with correct ownership at build time (no runtime chown needed)
RUN mkdir -p /etc/g3proxy /var/lib/g3proxy/ssl /tmp/g3 && \
    mkdir -p /var/log/g3proxy && \
    chown -R g3proxy:g3proxy /var/log/g3proxy /var/lib/g3proxy /tmp/g3

# Verify binaries
RUN which g3proxy && which g3fcgen && \
    g3proxy --version && g3fcgen --version

ENTRYPOINT ["/init.sh"]

