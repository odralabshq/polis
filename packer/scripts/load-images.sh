#!/bin/bash
# load-images.sh - Wait for Docker readiness and load pre-built images
# Addresses: Arch Review #2 (Docker readiness race)

set -euo pipefail

TIMEOUT=120
IMAGES_TAR="/tmp/polis-images.tar"

echo "==> Waiting for Docker daemon (timeout: ${TIMEOUT}s)..."

# Ensure Docker is started (Sysbox postinst may have stopped it)
sudo systemctl start docker 2>&1 || {
    echo "==> Docker start failed, checking status..." >&2
    sudo systemctl status docker --no-pager 2>&1 || true
    sudo journalctl -u docker --no-pager -n 30 2>&1 || true
    exit 1
}

# Wait for Docker daemon with timeout
end_time=$((SECONDS + TIMEOUT))
while ! sudo docker info >/dev/null 2>&1; do
    if [[ ${SECONDS} -ge ${end_time} ]]; then
        echo "ERROR: Docker daemon not ready after ${TIMEOUT}s" >&2
        sudo systemctl status docker --no-pager 2>&1 || true
        sudo journalctl -u docker --no-pager -n 40 2>&1 || true
        exit 1
    fi
    sleep 1
done

echo "==> Docker daemon ready"

# Load images
if [[ -f "${IMAGES_TAR}" ]]; then
    echo "==> Loading Docker images from ${IMAGES_TAR}..."
    sudo docker load -i "${IMAGES_TAR}"
    rm -f "${IMAGES_TAR}"
    echo "==> Images loaded successfully"
    sudo docker images
else
    echo "==> No images tar found at ${IMAGES_TAR}, skipping"
fi
