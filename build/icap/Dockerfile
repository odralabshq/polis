FROM debian:trixie AS builder
ENV DEBIAN_FRONTEND=noninteractive

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    autoconf \
    automake \
    libtool \
    wget \
    libssl-dev \
    libhiredis-dev \
    zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*

# Download and build c-icap from GitHub release (with SHA256 verification)
WORKDIR /build
ENV CICAP_SHA256=ecc7789787fe4eceae807f0717f6da18c5b1fbf7d2d26028711222eb154c82fe
RUN wget -q https://github.com/c-icap/c-icap-server/archive/refs/tags/C_ICAP_0.6.4.tar.gz -O c-icap.tar.gz && \
    echo "${CICAP_SHA256}  c-icap.tar.gz" | sha256sum -c - && \
    tar xzf c-icap.tar.gz && \
    cd c-icap-server-C_ICAP_0.6.4 && \
    autoreconf -i && \
    ./configure --prefix=/usr --sysconfdir=/etc --enable-large-files && \
    make -j$(nproc) && \
    make install DESTDIR=/install && \
    make install

# Download and build SquidClamav (with SHA256 verification)
ENV SQUIDCLAMAV_SHA256=772c058d113a17de0e96b3c8acaad75d78cd4c000f325b865d6c88503c353fc9
RUN wget -q https://github.com/darold/squidclamav/archive/refs/tags/v7.3.tar.gz -O squidclamav.tar.gz && \
    echo "${SQUIDCLAMAV_SHA256}  squidclamav.tar.gz" | sha256sum -c - && \
    tar xzf squidclamav.tar.gz && \
    cd squidclamav-7.3 && \
    ./configure --with-c-icap=/usr && \
    make -j$(nproc) && \
    make install DESTDIR=/install

# Copy and compile DLP module against c-ICAP source headers
COPY build/icap/srv_molis_dlp.c /build/
RUN gcc -shared -fPIC -Wall -Werror -o /build/srv_molis_dlp.so /build/srv_molis_dlp.c \
    -I/build/c-icap-server-C_ICAP_0.6.4 \
    -I/build/c-icap-server-C_ICAP_0.6.4/include \
    -L/usr/lib -licapapi -lhiredis

# Copy and compile approval REQMOD rewriter module
COPY build/icap/srv_molis_approval_rewrite.c /build/
RUN gcc -shared -fPIC -Wall -Werror \
    -o /build/srv_molis_approval_rewrite.so \
    /build/srv_molis_approval_rewrite.c \
    -I/build/c-icap-server-C_ICAP_0.6.4 \
    -I/build/c-icap-server-C_ICAP_0.6.4/include \
    -L/usr/lib -licapapi -lhiredis -lhiredis_ssl -lssl -lcrypto

# Copy and compile approval RESPMOD scanner module
COPY build/icap/srv_molis_approval.c /build/
RUN gcc -shared -fPIC -Wall -Werror \
    -o /build/srv_molis_approval.so \
    /build/srv_molis_approval.c \
    -I/build/c-icap-server-C_ICAP_0.6.4 \
    -I/build/c-icap-server-C_ICAP_0.6.4/include \
    -L/usr/lib -licapapi -lhiredis -lhiredis_ssl -lssl -lcrypto -lz

# Runtime image
FROM debian:trixie
ENV DEBIAN_FRONTEND=noninteractive

# Install runtime dependencies and gosu for privilege dropping
RUN apt-get update && apt-get install -y \
    libssl3 \
    zlib1g \
    libatomic1 \
    libhiredis1.1.0 \
    procps \
    gosu \
    netcat-openbsd \
    && rm -rf /var/lib/apt/lists/* \
    && gosu nobody true

# Copy built c-icap and squidclamav
COPY --from=builder /install/usr /usr
COPY --from=builder /install/etc /etc

# Copy DLP module
COPY --from=builder /build/srv_molis_dlp.so /usr/lib/c_icap/

# Copy approval modules
COPY --from=builder /build/srv_molis_approval_rewrite.so /usr/lib/c_icap/
COPY --from=builder /build/srv_molis_approval.so /usr/lib/c_icap/

# Create dedicated c-icap user and group
RUN groupadd -r c-icap && useradd -r -g c-icap -s /sbin/nologin c-icap

# Create directories with proper ownership
RUN mkdir -p /var/run/c-icap /run/c-icap /usr/var/log && \
    chown c-icap:c-icap /var/run/c-icap /run/c-icap /usr/var/log && \
    chmod 755 /var/run/c-icap /run/c-icap /usr/var/log

# Entrypoint with gosu for safe privilege dropping
COPY <<'EOF' /entrypoint.sh
#!/bin/bash
set -e
mkdir -p /var/run/c-icap
chown c-icap:c-icap /var/run/c-icap
chmod 755 /var/run/c-icap
rm -f /var/run/c-icap/c-icap.pid /var/run/c-icap/c-icap.ctl
echo "[icap] Starting c-icap with SquidClamav on 0.0.0.0:1344..."
exec gosu c-icap /usr/bin/c-icap -N -D -d 1 -f /etc/c-icap/c-icap.conf
EOF
RUN sed -i 's/\r$//' /entrypoint.sh && chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]