# hadolint ignore=DL3007
# Root required: sysbox VM container runs systemd which needs root.
# Actual isolation provided by sysbox-runc runtime (VM-level, not container-level).
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
    openssh-server \
    netcat-openbsd \
    libnss-systemd \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /run/sshd /var/empty \
    && chmod 755 /var/empty

# Configure NSS for user/group resolution (required for sshd privilege separation)
# Configure sshd: disable PAM (minimal image lacks common-auth), key-only auth
RUN printf '%s\n' \
    'passwd:     files systemd' \
    'group:      files systemd' \
    'shadow:     files' \
    'hosts:      files dns' \
    'networks:   files' \
    'protocols:  files' \
    'services:   files' \
    'ethers:     files' \
    'rpc:        files' \
    > /etc/nsswitch.conf && \
    printf '%s\n' \
    'UsePAM no' \
    'PasswordAuthentication no' \
    'PermitEmptyPasswords no' \
    'ChallengeResponseAuthentication no' \
    'PubkeyAuthentication yes' \
    'AuthorizedKeysFile .ssh/authorized_keys' \
    > /etc/ssh/sshd_config.d/polis.conf

# Create polis user (uid 1000) and disable root login
RUN useradd -m -u 1000 -s /bin/bash polis \
    && usermod -s /usr/sbin/nologin root

# Enable polis-init service (service file mounted at runtime)
RUN ln -sf /etc/systemd/system/polis-init.service /etc/systemd/system/multi-user.target.wants/polis-init.service

ENTRYPOINT ["/sbin/init"]
