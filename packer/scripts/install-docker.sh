#!/bin/bash
# install-docker.sh - Install Docker via apt repository with GPG fingerprint verification
# Addresses: V2 (no curl|sh), Arch Review #3 (no errant jq)

set -euo pipefail

DOCKER_GPG_FINGERPRINT="9DC858229FC7DD38854AE2D88D81803C0EBFCD88"

echo "==> Installing Docker via apt repository..."

# Install prerequisites
sudo apt-get update
sudo apt-get install -y ca-certificates curl gnupg

# Download and verify Docker GPG key
sudo install -m 0755 -d /etc/apt/keyrings
curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo tee /etc/apt/keyrings/docker.asc > /dev/null
sudo chmod a+r /etc/apt/keyrings/docker.asc

# Verify GPG fingerprint
ACTUAL_FINGERPRINT=$(gpg --show-keys --with-fingerprint /etc/apt/keyrings/docker.asc 2>/dev/null | grep -oP '[A-F0-9]{40}' | head -1)
if [[ "${ACTUAL_FINGERPRINT}" != "${DOCKER_GPG_FINGERPRINT}" ]]; then
    echo "ERROR: Docker GPG fingerprint mismatch!" >&2
    echo "  Expected: ${DOCKER_GPG_FINGERPRINT}" >&2
    echo "  Actual:   ${ACTUAL_FINGERPRINT}" >&2
    exit 1
fi
echo "==> Docker GPG fingerprint verified: ${DOCKER_GPG_FINGERPRINT}"

# Add Docker apt repository
# shellcheck disable=SC1091
echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu $(. /etc/os-release && echo "${VERSION_CODENAME}") stable" | \
    sudo tee /etc/apt/sources.list.d/docker.list > /dev/null

# Install Docker packages
sudo apt-get update
sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin

# Add ubuntu user to docker group
sudo usermod -aG docker ubuntu

echo "==> Docker installed successfully"
docker --version
