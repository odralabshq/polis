#!/bin/bash
# test-update-local.sh — Test polis update container flow locally
# Serves a signed versions.json from a local HTTP server so no GitHub access needed.
#
# Usage: ./tools/test-update-local.sh [version]
#   version: version string to put in manifest (default: v0.3.1)
#
# Requires: python3, zipsign, cargo-built polis binary
set -euo pipefail

VERSION="${1:-v0.3.1}"
SIGNING_KEY=".secrets/polis-release.key"
SERVE_DIR=$(mktemp -d)
PORT=19876
trap 'rm -rf "${SERVE_DIR}"; kill "${SERVER_PID}" 2>/dev/null || true' EXIT

if [[ ! -f "${SIGNING_KEY}" ]]; then
    echo "No signing key found — generating throwaway keypair for local testing..."
    zipsign gen-key "${SERVE_DIR}/test.key" "${SERVE_DIR}/test.pub"
    SIGNING_KEY="${SERVE_DIR}/test.key"
    TEST_VERIFYING_KEY_B64=$(base64 -w0 "${SERVE_DIR}/test.pub")
else
    TEST_VERIFYING_KEY_B64=$(base64 -w0 "${SIGNING_KEY%.key}.pub" 2>/dev/null || base64 -w0 .secrets/polis-release.pub 2>/dev/null || "")
fi

# Generate versions.json
cat > "${SERVE_DIR}/versions.json.plain" << EOF
{
  "manifest_version": 1,
  "vm_image": {
    "version": "${VERSION}",
    "asset": "polis-workspace-${VERSION}-amd64.qcow2"
  },
  "containers": {
    "polis-resolver-oss": "${VERSION}",
    "polis-certgen-oss": "${VERSION}",
    "polis-gate-oss": "${VERSION}",
    "polis-sentinel-oss": "${VERSION}",
    "polis-scanner-oss": "${VERSION}",
    "polis-workspace-oss": "${VERSION}",
    "polis-host-init-oss": "${VERSION}",
    "polis-state-oss": "${VERSION}",
    "polis-toolbox-oss": "${VERSION}"
  }
}
EOF

# Sign it
tar -czf "${SERVE_DIR}/versions.json.tar.gz" -C "${SERVE_DIR}" versions.json.plain
mv "${SERVE_DIR}/versions.json.plain" "${SERVE_DIR}/versions.json.src"
zipsign sign tar "${SERVE_DIR}/versions.json.tar.gz" "${SIGNING_KEY}" \
    -o "${SERVE_DIR}/versions.json" -f
rm "${SERVE_DIR}/versions.json.tar.gz" "${SERVE_DIR}/versions.json.src"

# GitHub API mock: releases array with one release containing the versions.json asset
DOWNLOAD_URL="http://127.0.0.1:${PORT}/versions.json"
cat > "${SERVE_DIR}/releases.json" << EOF
[{
  "tag_name": "${VERSION}",
  "assets": [{
    "name": "versions.json",
    "browser_download_url": "${DOWNLOAD_URL}"
  }]
}]
EOF

# Serve
python3 -m http.server "${PORT}" --directory "${SERVE_DIR}" --bind 127.0.0.1 &>/dev/null &
SERVER_PID=$!
sleep 0.5

echo "Local update server running on port ${PORT} (version: ${VERSION})"
echo "Running: POLIS_GITHUB_API_URL=http://127.0.0.1:${PORT}/releases.json polis update"
echo ""

POLIS_GITHUB_API_URL="http://127.0.0.1:${PORT}/releases.json" \
POLIS_VERIFYING_KEY_B64="${TEST_VERIFYING_KEY_B64}" \
    cargo run --manifest-path cli/Cargo.toml --quiet -- update
