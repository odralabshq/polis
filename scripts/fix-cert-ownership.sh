#!/bin/bash
set -euo pipefail

# =============================================================================
# Polis Certificate Ownership Fixer
# Fixes ownership of TLS private keys so containers can read them.
#
# Security rationale — CA key isolation:
#   The CA private key (ca.key) is mounted ONLY into the certgen sidecar,
#   which runs as uid 65532 and needs it to sign certificates on-the-fly.
#   No other container mounts ca.key — they only receive ca.pem for TLS
#   verification. This means that even if a non-certgen container is
#   compromised, it cannot forge certificates for other services.
#
# Asymmetric tolerance — Valkey required, Toolbox optional:
#   Valkey keys (server.key, client.key) are REQUIRED. If they are missing it
#   indicates broken cert generation upstream and the script fails loudly rather
#   than silently leaving services unable to start.
#   Toolbox key (toolbox.key) is OPTIONAL because the Toolbox service is not
#   present in all installations (e.g. minimal or headless deployments). A
#   missing toolbox.key is tolerated with || true so the script does not block
#   installations that legitimately omit Toolbox.
# =============================================================================

# Polis root directory (default: /opt/polis)
POLIS_ROOT="${1:-/opt/polis}"

echo "=== Polis Certificate Ownership Fixer ==="
echo "Polis root: ${POLIS_ROOT}"

# ---------------------------------------------------------------------------
# CA key — 65532:65532 mode 600
# The certgen sidecar (uid 65532) is the ONLY container that mounts ca.key.
# It needs read access to sign certificates on-the-fly. All other containers
# only mount ca.pem (the public cert) for TLS verification.
# ---------------------------------------------------------------------------
echo ""
echo "--- Fixing CA key ownership (65532:65532, mode 600) ---"
chown 65532:65532 "${POLIS_ROOT}/certs/ca/ca.key"
chmod 600 "${POLIS_ROOT}/certs/ca/ca.key"

# ---------------------------------------------------------------------------
# Valkey keys — 65532:65532 (REQUIRED)
# Both server.key and client.key must exist. Missing keys indicate that cert
# generation failed upstream; fail loudly here rather than letting services
# start with broken TLS.
# The Valkey CA key is also chowned so the entire certs/valkey directory is
# accessible to the container (mounted as a volume).
# ---------------------------------------------------------------------------
echo ""
echo "--- Fixing Valkey key ownership (65532:65532) [REQUIRED] ---"
if [[ ! -f "${POLIS_ROOT}/certs/valkey/server.key" ]]; then
    echo "ERROR: ${POLIS_ROOT}/certs/valkey/server.key is missing." >&2
    echo "       Valkey cert generation may have failed. Aborting." >&2
    exit 1
fi
if [[ ! -f "${POLIS_ROOT}/certs/valkey/client.key" ]]; then
    echo "ERROR: ${POLIS_ROOT}/certs/valkey/client.key is missing." >&2
    echo "       Valkey cert generation may have failed. Aborting." >&2
    exit 1
fi
chown 65532:65532 "${POLIS_ROOT}/certs/valkey/server.key"
chown 65532:65532 "${POLIS_ROOT}/certs/valkey/client.key"
chown 65532:65532 "${POLIS_ROOT}/certs/valkey/ca.key" 2>/dev/null || true

# ---------------------------------------------------------------------------
# Toolbox key — 65532:65532 (OPTIONAL)
# Toolbox is not present in all installations. Tolerate a missing key with
# || true so minimal/headless deployments are not blocked.
# ---------------------------------------------------------------------------
echo ""
echo "--- Fixing Toolbox key ownership (65532:65532) [OPTIONAL] ---"
chown 65532:65532 "${POLIS_ROOT}/certs/toolbox/toolbox.key" || true

# ---------------------------------------------------------------------------
# Sentinel — signals that the entire cert chain completed successfully.
# Doctor repair checks for this file rather than individual cert files to
# catch partial failures where some certs exist but others are missing.
# ---------------------------------------------------------------------------
echo ""
echo "--- Writing cert-chain completion sentinel ---"
touch "${POLIS_ROOT}/.certs-ready"

echo ""
echo "=== Certificate ownership fix complete ==="
echo "Sentinel written: ${POLIS_ROOT}/.certs-ready"
