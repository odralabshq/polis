#!/bin/bash
set -euo pipefail

# =============================================================================
# Polis CA Certificate Generator
# Generates a 4096-bit RSA CA key and self-signed x509 certificate
# =============================================================================

# CA output directory (default: ./certs/ca)
CA_DIR="${1:-./certs/ca}"

echo "=== Polis CA Certificate Generator ==="
echo "CA directory: ${CA_DIR}"

# Idempotency: skip if both key and cert already exist
if [[ -f "${CA_DIR}/ca.key" ]] && [[ -f "${CA_DIR}/ca.pem" ]]; then
    echo "CA certificate already exists. Skipping."
    exit 0
fi

# Cleanup any partially-written files on error or interrupt
trap 'rm -f "${CA_DIR}/ca.key" "${CA_DIR}/ca.pem"' ERR

mkdir -p "${CA_DIR}"

echo ""
echo "--- Generating 4096-bit RSA CA key ---"
openssl genrsa -out "${CA_DIR}/ca.key" 4096 2>/dev/null

echo "--- Generating self-signed CA certificate (10-year validity) ---"
openssl req -new -x509 \
    -days 3650 \
    -key "${CA_DIR}/ca.key" \
    -out "${CA_DIR}/ca.pem" \
    -subj "/C=US/ST=Local/L=Local/O=Polis/OU=Gateway/CN=Polis CA" \
    2>/dev/null

# Set permissions: key=600 (owner only), cert=644 (public)
chmod 600 "${CA_DIR}/ca.key"
chmod 644 "${CA_DIR}/ca.pem"

echo ""
echo "=== CA generation complete ==="
echo "Files created in: ${CA_DIR}"
echo "  ca.key  (private key, mode 600)"
echo "  ca.pem  (certificate, mode 644)"
