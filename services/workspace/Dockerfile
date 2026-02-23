# hadolint ignore=DL3007
# Root required: sysbox VM container runs systemd which needs root.
# Actual isolation provided by sysbox-runc runtime (VM-level, not container-level).
FROM dhi.io/debian-base:trixie-dev@sha256:2166e2eaef0651c9ad21de6ab5a34fda12541d89bccf7bcb0a94afceb1b1541b

# Install systemd and required tools for networking
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    iproute2 \
    kmod \
    libnss-systemd \
    netcat-openbsd \
    openssh-server \
    passwd \
    procps \
    systemd \
    systemd-sysv \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /run/sshd /var/empty \
    && chmod 755 /var/empty

# Development toolchains: Python, Node.js
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    cmake \
    diffutils \
    fd-find \
    g++ \
    gcc \
    htop \
    jq \
    less \
    make \
    nodejs \
    npm \
    patch \
    python3 \
    python3-pip \
    python3-venv \
    ripgrep \
    tree \
    unzip \
    vim \
    wget \
    zip \
    && mkdir -p /usr/local/bin && ln -s /usr/bin/fdfind /usr/local/bin/fd \
    && rm -rf /var/lib/apt/lists/*

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
    > /etc/ssh/sshd_config.d/polis.conf \
    && useradd -m -u 1000 -s /bin/bash polis \
    && usermod -s /usr/sbin/nologin root \
    && ln -sf /etc/systemd/system/polis-init.service /etc/systemd/system/multi-user.target.wants/polis-init.service \
    && mkdir -p /usr/local/share/ca-certificates

# Rust via rustup (installed as polis user, not root)
USER polis
SHELL ["/bin/bash", "-o", "pipefail", "-c"]
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
SHELL ["/bin/sh", "-c"]

# Switch back to root for systemd entrypoint (/sbin/init requires PID 1 as root).
# Security context: this container runs under Sysbox runtime with user-namespace
# isolation — root inside the container is NOT root on the host.
# hadolint ignore=DL3002
USER root
ENV PATH="/home/polis/.cargo/bin:${PATH}"

COPY --chmod=755 services/workspace/scripts/init.sh /usr/local/bin/init.sh

# nosemgrep: dockerfile.security.last-user-is-root.last-user-is-root
# checkov:skip=CKV_DOCKER_3: systemd entrypoint requires root; Sysbox provides user-namespace isolation
ENTRYPOINT ["/sbin/init"]
