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
    libdb-dev \
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

# Download and build c-icap-modules (contains srv_url_check)
ENV CICAP_MODULES_SHA256=a7ea38d09888920da12167d4f26669bbc454401561570e24a63af52f9f17cadc
RUN wget -q https://github.com/c-icap/c-icap-modules/archive/refs/tags/C_ICAP_MODULES_0.5.7.tar.gz -O c-icap-modules.tar.gz && \
    echo "${CICAP_MODULES_SHA256}  c-icap-modules.tar.gz" | sha256sum -c - && \
    tar xzf c-icap-modules.tar.gz && \
    cd c-icap-modules-C_ICAP_MODULES_0.5.7 && \
    autoreconf -i && \
    ./configure --with-c-icap=/usr --prefix=/usr && \
    make -j$(nproc) && \
    make install DESTDIR=/install

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
COPY services/sentinel/modules/dlp/srv_polis_dlp.c /build/
RUN gcc -shared -fPIC -Wall -Werror -o /build/srv_polis_dlp.so /build/srv_polis_dlp.c \
    -I/build/c-icap-server-C_ICAP_0.6.4 \
    -I/build/c-icap-server-C_ICAP_0.6.4/include \
    -L/usr/lib -licapapi -lhiredis -lhiredis_ssl -lssl -lcrypto -lpthread

# Copy and compile approval REQMOD rewriter module
COPY services/sentinel/modules/approval/srv_polis_approval_rewrite.c /build/
RUN gcc -shared -fPIC -Wall -Werror \
    -o /build/srv_polis_approval_rewrite.so \
    /build/srv_polis_approval_rewrite.c \
    -I/build/c-icap-server-C_ICAP_0.6.4 \
    -I/build/c-icap-server-C_ICAP_0.6.4/include \
    -L/usr/lib -licapapi -lhiredis -lhiredis_ssl -lssl -lcrypto

# Copy and compile approval RESPMOD scanner module
COPY services/sentinel/modules/approval/srv_polis_approval.c /build/
RUN gcc -shared -fPIC -Wall -Werror \
    -o /build/srv_polis_approval.so \
    /build/srv_polis_approval.c \
    -I/build/c-icap-server-C_ICAP_0.6.4 \
    -I/build/c-icap-server-C_ICAP_0.6.4/include \
    -L/usr/lib -licapapi -lhiredis -lhiredis_ssl -lssl -lcrypto -lz

# Copy and compile merged Sentinel RESPMOD module (ClamAV + OTT)
COPY services/sentinel/modules/merged/srv_polis_sentinel_resp.c /build/
RUN gcc -shared -fPIC -Wall -Werror \
    -o /build/srv_polis_sentinel_resp.so \
    /build/srv_polis_sentinel_resp.c \
    -I/build/c-icap-server-C_ICAP_0.6.4 \
    -I/build/c-icap-server-C_ICAP_0.6.4/include \
    -L/usr/lib -licapapi -lhiredis -lhiredis_ssl -lssl -lcrypto -lpthread -lz

# Runtime image
FROM debian:trixie
ENV DEBIAN_FRONTEND=noninteractive

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl3 \
    zlib1g \
    libatomic1 \
    libhiredis1.1.0 \
    procps \
    netcat-openbsd \
    && rm -rf /var/lib/apt/lists/*

# Copy built c-icap and squidclamav
COPY --from=builder /install/usr /usr
COPY --from=builder /install/etc /etc

# Copy DLP module
COPY --from=builder /build/srv_polis_dlp.so /usr/lib/c_icap/

# Copy approval modules
COPY --from=builder /build/srv_polis_approval_rewrite.so /usr/lib/c_icap/
COPY --from=builder /build/srv_polis_approval.so /usr/lib/c_icap/

# Copy merged Sentinel RESPMOD module
COPY --from=builder /build/srv_polis_sentinel_resp.so /usr/lib/c_icap/

# Create dedicated c-icap user and group
# Create dedicated sentinel user and group (uid/gid 101 matches sentinel tmpfs in compose)
RUN groupadd -r -g 101 c-icap && useradd -r -u 101 -g c-icap -s /sbin/nologin c-icap && \
    usermod -l sentinel -d /home/sentinel -m c-icap && \
    groupmod -n sentinel c-icap

# Create directories with proper ownership
RUN mkdir -p /var/run/c-icap /run/c-icap /var/log/c-icap && \
    chown sentinel:sentinel /var/run/c-icap /run/c-icap /var/log/c-icap && \
    chmod 755 /var/run/c-icap /run/c-icap /var/log/c-icap

# Entrypoint (non-root)
COPY services/sentinel/scripts/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

USER sentinel
ENTRYPOINT ["/entrypoint.sh"]