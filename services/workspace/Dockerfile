# hadolint ignore=DL3007
# Root required: sysbox VM container runs systemd which needs root.
# Actual isolation provided by sysbox-runc runtime (VM-level, not container-level).
FROM dhi.io/debian-base:trixie-dev@sha256:2166e2eaef0651c9ad21de6ab5a34fda12541d89bccf7bcb0a94afceb1b1541b

# Single layer: systemd, networking, dev tools, user setup
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    diffutils \
    fd-find \
    git \
    htop \
    iproute2 \
    jq \
    kmod \
    less \
    libnss-systemd \
    netcat-openbsd \
    nodejs \
    npm \
    openssh-server \
    passwd \
    patch \
    procps \
    python3 \
    python3-pip \
    python3-venv \
    ripgrep \
    systemd \
    systemd-sysv \
    tree \
    unzip \
    wget \
    zip \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /run/sshd /var/empty /usr/local/bin /usr/local/share/ca-certificates \
    && chmod 755 /var/empty \
    && ln -s /usr/bin/fdfind /usr/local/bin/fd \
    && printf '%s\n' \
        'passwd:     files systemd' \
        'group:      files systemd' \
        'shadow:     files' \
        'hosts:      files dns' \
        'networks:   files' \
        'protocols:  files' \
        'services:   files' \
        'ethers:     files' \
        'rpc:        files' \
        > /etc/nsswitch.conf \
    && printf '%s\n' \
        'UsePAM no' \
        'PasswordAuthentication no' \
        'PermitEmptyPasswords no' \
        'ChallengeResponseAuthentication no' \
        'PubkeyAuthentication yes' \
        'AuthorizedKeysFile .ssh/authorized_keys' \
        > /etc/ssh/sshd_config.d/polis.conf \
    && useradd -m -u 1000 -s /bin/bash polis \
    && usermod -s /usr/sbin/nologin root \
    && ln -sf /etc/systemd/system/polis-init.service /etc/systemd/system/multi-user.target.wants/polis-init.service

COPY --chmod=755 services/workspace/scripts/init.sh /usr/local/bin/init.sh

# hadolint ignore=DL3002
USER root

# nosemgrep: dockerfile.security.last-user-is-root.last-user-is-root
# checkov:skip=CKV_DOCKER_3: systemd entrypoint requires root; Sysbox provides user-namespace isolation
ENTRYPOINT ["/sbin/init"]
