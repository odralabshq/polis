FROM dhi.io/debian-base:trixie-dev@sha256:2166e2eaef0651c9ad21de6ab5a34fda12541d89bccf7bcb0a94afceb1b1541b AS builder
ENV DEBIAN_FRONTEND=noninteractive
SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    autoconf \
    automake \
    libtool \
    wget \
    libssl-dev \
    libhiredis-dev \
    zlib1g-dev \
    libdb-dev \
    && rm -rf /var/lib/apt/lists/*

# OpenSSF compiler hardening flags for C modules
ENV CFLAGS_HARDEN="-fstack-protector-strong -D_FORTIFY_SOURCE=3 -Wformat -Wformat-security -fstack-clash-protection -O2"
ENV LDFLAGS_HARDEN="-Wl,-z,relro,-z,now"

# Download and build c-icap from GitHub release (with SHA256 verification)
WORKDIR /build
ENV CICAP_SHA256=ecc7789787fe4eceae807f0717f6da18c5b1fbf7d2d26028711222eb154c82fe
RUN wget -q https://github.com/c-icap/c-icap-server/archive/refs/tags/C_ICAP_0.6.4.tar.gz -O c-icap.tar.gz && \
    echo "${CICAP_SHA256}  c-icap.tar.gz" | sha256sum -c - && \
    tar xzf c-icap.tar.gz

WORKDIR /build/c-icap-server-C_ICAP_0.6.4
RUN autoreconf -i && \
    ./configure --prefix=/usr/local --sysconfdir=/etc --enable-large-files && \
    make -j"$(nproc)" && \
    make install DESTDIR=/install && \
    make install

# Download and build c-icap-modules (contains srv_url_check)
ENV CICAP_MODULES_SHA256=a7ea38d09888920da12167d4f26669bbc454401561570e24a63af52f9f17cadc
WORKDIR /build
RUN wget -q https://github.com/c-icap/c-icap-modules/archive/refs/tags/C_ICAP_MODULES_0.5.7.tar.gz -O c-icap-modules.tar.gz && \
    echo "${CICAP_MODULES_SHA256}  c-icap-modules.tar.gz" | sha256sum -c - && \
    tar xzf c-icap-modules.tar.gz

WORKDIR /build/c-icap-modules-C_ICAP_MODULES_0.5.7
RUN autoreconf -i && \
    ./configure --with-c-icap=/usr/local --prefix=/usr/local && \
    make -j"$(nproc)" && \
    make install DESTDIR=/install

# Download and build SquidClamav (with SHA256 verification)
ENV SQUIDCLAMAV_SHA256=772c058d113a17de0e96b3c8acaad75d78cd4c000f325b865d6c88503c353fc9
WORKDIR /build
RUN wget -q https://github.com/darold/squidclamav/archive/refs/tags/v7.3.tar.gz -O squidclamav.tar.gz && \
    echo "${SQUIDCLAMAV_SHA256}  squidclamav.tar.gz" | sha256sum -c - && \
    tar xzf squidclamav.tar.gz

WORKDIR /build/squidclamav-7.3
RUN ./configure --with-c-icap=/usr/local && \
    make -j"$(nproc)" && \
    make install DESTDIR=/install

# Copy and compile DLP module against c-ICAP source headers
WORKDIR /build
COPY services/sentinel/modules/dlp/srv_polis_dlp.c /build/
RUN gcc -shared -fPIC -Wall -Werror ${CFLAGS_HARDEN} ${LDFLAGS_HARDEN} \
    -o /build/srv_polis_dlp.so /build/srv_polis_dlp.c \
    -I/build/c-icap-server-C_ICAP_0.6.4 \
    -I/build/c-icap-server-C_ICAP_0.6.4/include \
    -L/usr/local/lib -licapapi -lhiredis -lhiredis_ssl -lssl -lcrypto -lpthread

# Copy and compile approval REQMOD rewriter module
COPY services/sentinel/modules/approval/srv_polis_approval_rewrite.c /build/
RUN gcc -shared -fPIC -Wall -Werror ${CFLAGS_HARDEN} ${LDFLAGS_HARDEN} \
    -o /build/srv_polis_approval_rewrite.so \
    /build/srv_polis_approval_rewrite.c \
    -I/build/c-icap-server-C_ICAP_0.6.4 \
    -I/build/c-icap-server-C_ICAP_0.6.4/include \
    -L/usr/local/lib -licapapi -lhiredis -lhiredis_ssl -lssl -lcrypto

# Copy and compile approval RESPMOD scanner module
COPY services/sentinel/modules/approval/srv_polis_approval.c /build/
RUN gcc -shared -fPIC -Wall -Werror ${CFLAGS_HARDEN} ${LDFLAGS_HARDEN} \
    -o /build/srv_polis_approval.so \
    /build/srv_polis_approval.c \
    -I/build/c-icap-server-C_ICAP_0.6.4 \
    -I/build/c-icap-server-C_ICAP_0.6.4/include \
    -L/usr/local/lib -licapapi -lhiredis -lhiredis_ssl -lssl -lcrypto -lz

# Copy and compile merged Sentinel RESPMOD module (ClamAV + OTT)
COPY services/sentinel/modules/merged/srv_polis_sentinel_resp.c /build/
RUN gcc -shared -fPIC -Wall -Werror ${CFLAGS_HARDEN} ${LDFLAGS_HARDEN} \
    -o /build/srv_polis_sentinel_resp.so \
    /build/srv_polis_sentinel_resp.c \
    -I/build/c-icap-server-C_ICAP_0.6.4 \
    -I/build/c-icap-server-C_ICAP_0.6.4/include \
    -L/usr/local/lib -licapapi -lhiredis -lhiredis_ssl -lssl -lcrypto -lpthread -lz

# Collect runtime shared libraries not in base image
RUN mkdir -p /runtime-libs/lib/x86_64-linux-gnu /runtime-libs/usr/lib/x86_64-linux-gnu && \
    cp -L /lib/x86_64-linux-gnu/libatomic.so.1 \
          /lib/x86_64-linux-gnu/libhiredis.so.1.1.0 \
          /lib/x86_64-linux-gnu/libhiredis_ssl.so.1.1.0 \
          /runtime-libs/lib/x86_64-linux-gnu/ && \
    cp -L /usr/lib/x86_64-linux-gnu/libdb-5.3.so \
          /runtime-libs/usr/lib/x86_64-linux-gnu/

# Runtime image (minimal, no apt)
FROM dhi.io/debian-base:trixie@sha256:9ef766670af4743904b0f992a26b525c6c914648b56ea597ec23452adcf1a95d

# Copy runtime shared libraries not in base
COPY --from=builder /runtime-libs/lib/ /lib/
COPY --from=builder /runtime-libs/usr/lib/ /usr/lib/

# Copy built c-icap and squidclamav
COPY --from=builder /install/usr /usr
COPY --from=builder /install/etc /etc

# Copy DLP module
COPY --from=builder /build/srv_polis_dlp.so /usr/local/lib/c_icap/

# Copy approval modules
COPY --from=builder /build/srv_polis_approval_rewrite.so /usr/local/lib/c_icap/
COPY --from=builder /build/srv_polis_approval.so /usr/local/lib/c_icap/

# Copy merged Sentinel RESPMOD module
COPY --from=builder /build/srv_polis_sentinel_resp.so /usr/local/lib/c_icap/

# Create directories with proper ownership for DHI nonroot user (65532)
USER 0
RUN mkdir -p /var/run/c-icap /run/c-icap /var/log/c-icap && \
    chown 65532:65532 /var/run/c-icap /run/c-icap /var/log/c-icap && \
    chmod 755 /var/run/c-icap /run/c-icap /var/log/c-icap

# Entrypoint (non-root)
COPY services/sentinel/scripts/entrypoint.sh /entrypoint.sh
RUN chmod 755 /entrypoint.sh

# Ensure linker finds c-icap libs at /usr/local/lib
ENV LD_LIBRARY_PATH=/usr/local/lib

USER 65532
ENTRYPOINT ["/entrypoint.sh"]