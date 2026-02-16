# hadolint ignore=DL3007
# Root required: sysbox VM container runs systemd which needs root.
# Actual isolation provided by sysbox-runc runtime (VM-level, not container-level).
FROM debian:trixie

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
