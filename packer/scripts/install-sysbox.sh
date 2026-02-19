#!/bin/bash
# install-sysbox.sh - Install Sysbox with SHA256 verification
# Addresses: V3 (SHA256 check), Arch Review #3 (no errant jq in apt-get)

set -euo pipefail

: "${SYSBOX_VERSION:?SYSBOX_VERSION required}"
: "${SYSBOX_SHA256:?SYSBOX_SHA256 required}"
: "${ARCH:?ARCH required}"

DEB_NAME="sysbox-ce_${SYSBOX_VERSION}.linux_${ARCH}.deb"
DEB_URL="https://github.com/nestybox/sysbox/releases/download/v${SYSBOX_VERSION}/${DEB_NAME}"

echo "==> Installing Sysbox ${SYSBOX_VERSION} (${ARCH})..."

# Download Sysbox .deb
curl -fsSL -o "/tmp/${DEB_NAME}" "${DEB_URL}"

# Verify SHA256
ACTUAL_SHA256=$(sha256sum "/tmp/${DEB_NAME}" | awk '{print $1}')
if [[ "${ACTUAL_SHA256}" != "${SYSBOX_SHA256}" ]]; then
    echo "ERROR: Sysbox SHA256 mismatch!" >&2
    echo "  Expected: ${SYSBOX_SHA256}" >&2
    echo "  Actual:   ${ACTUAL_SHA256}" >&2
    rm -f "/tmp/${DEB_NAME}"
    exit 1
fi
echo "==> Sysbox SHA256 verified: ${SYSBOX_SHA256}"

# Install Sysbox (no errant jq - Arch Review #3)
sudo apt-get install -y "/tmp/${DEB_NAME}"

# Cleanup
rm -f "/tmp/${DEB_NAME}"

echo "==> Sysbox installed successfully"
