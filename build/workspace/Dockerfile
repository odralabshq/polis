FROM debian:trixie

# Create polis user (uid 1000)
RUN useradd -m -u 1000 -s /bin/bash polis \
    && chsh -s /usr/sbin/nologin root

# Install systemd and required tools for networking
RUN apt-get update && apt-get install -y --no-install-recommends \
    systemd \
    systemd-sysv \
    iproute2 \
    ca-certificates \
    curl \
    kmod \
    procps \
    && rm -rf /var/lib/apt/lists/*

# Enable polis-init service (service file mounted at runtime)
RUN ln -sf /etc/systemd/system/polis-init.service /etc/systemd/system/multi-user.target.wants/polis-init.service

ENTRYPOINT ["/sbin/init"]
