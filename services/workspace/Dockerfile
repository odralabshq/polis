FROM dhi.io/debian-base:trixie-dev@sha256:2166e2eaef0651c9ad21de6ab5a34fda12541d89bccf7bcb0a94afceb1b1541b

# Install systemd and required tools for networking
RUN apt-get update && apt-get install -y --no-install-recommends \
    systemd \
    systemd-sysv \
    iproute2 \
    ca-certificates \
    curl \
    kmod \
    procps \
    passwd \
    && rm -rf /var/lib/apt/lists/*

# Create polis user (uid 1000) and disable root login
RUN useradd -m -u 1000 -s /bin/bash polis \
    && usermod -s /usr/sbin/nologin root

# Enable polis-init service (service file mounted at runtime)
RUN ln -sf /etc/systemd/system/polis-init.service /etc/systemd/system/multi-user.target.wants/polis-init.service

ENTRYPOINT ["/sbin/init"]
