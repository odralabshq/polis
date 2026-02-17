#!/bin/bash
# load-images.sh - Wait for Docker readiness and load pre-built images
# Addresses: Arch Review #2 (Docker readiness race)

set -euo pipefail

TIMEOUT=30
IMAGES_TAR="/tmp/polis-images.tar"

echo "==> Waiting for Docker daemon (timeout: ${TIMEOUT}s)..."

# Wait for Docker daemon with timeout
end_time=$((SECONDS + TIMEOUT))
while ! docker info >/dev/null 2>&1; do
    if [[ ${SECONDS} -ge ${end_time} ]]; then
        echo "ERROR: Docker daemon not ready after ${TIMEOUT}s" >&2
        exit 1
    fi
    sleep 1
done

echo "==> Docker daemon ready"

# Load images
if [[ -f "${IMAGES_TAR}" ]]; then
    echo "==> Loading Docker images from ${IMAGES_TAR}..."
    docker load -i "${IMAGES_TAR}"
    rm -f "${IMAGES_TAR}"
    echo "==> Images loaded successfully"
    docker images
else
    echo "==> No images tar found at ${IMAGES_TAR}, skipping"
fi
