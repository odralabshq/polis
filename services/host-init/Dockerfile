# Host init container — configures host iptables for TPROXY
# checkov:skip=CKV_DOCKER_2:Init container runs once and exits, HEALTHCHECK not applicable
# checkov:skip=CKV_DOCKER_3:Init container requires root for iptables configuration
FROM dhi.io/debian-base:trixie-dev@sha256:2166e2eaef0651c9ad21de6ab5a34fda12541d89bccf7bcb0a94afceb1b1541b
RUN apt-get update && apt-get install -y --no-install-recommends docker.io iptables \
    && rm -rf /var/lib/apt/lists/*
