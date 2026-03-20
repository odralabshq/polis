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

# Cleanup any partially-written files on error or interrupt
trap 'rm -f "${CA_DIR}/ca.key" "${CA_DIR}/ca.pem"' ERR

mkdir -p "${CA_DIR}"

echo ""
echo "--- Generating 4096-bit RSA CA key ---"
openssl genrsa -out "${CA_DIR}/ca.key" 4096 2>/dev/null

echo "--- Generating self-signed CA certificate (10-year validity) ---"
# Write a temporary extensions file so the CA cert includes keyUsage and
# basicConstraints.  Python 3.13+ (OpenSSL 3.x) rejects CA certificates
# that lack the keyUsage extension with keyCertSign.
CA_EXT=$(mktemp)
trap 'rm -f "${CA_EXT}"; rm -f "${CA_DIR}/ca.key" "${CA_DIR}/ca.pem"' ERR
cat > "${CA_EXT}" <<'EOF'
[v3_ca]
basicConstraints = critical, CA:TRUE
keyUsage         = critical, keyCertSign, cRLSign
subjectKeyIdentifier   = hash
authorityKeyIdentifier = keyid:always, issuer
EOF

openssl req -new -x509 \
    -days 3650 \
    -key "${CA_DIR}/ca.key" \
    -out "${CA_DIR}/ca.pem" \
    -subj "/C=US/ST=Local/L=Local/O=Polis/OU=Gateway/CN=Polis CA" \
    -extensions v3_ca \
    -config <(cat /etc/ssl/openssl.cnf "${CA_EXT}" 2>/dev/null || cat "${CA_EXT}") \
    2>/dev/null

rm -f "${CA_EXT}"

# Set permissions: key=600 (owner only), cert=644 (public)
chmod 600 "${CA_DIR}/ca.key"
chmod 644 "${CA_DIR}/ca.pem"

echo ""
echo "=== CA generation complete ==="
echo "Files created in: ${CA_DIR}"
echo "  ca.key  (private key, mode 600)"
echo "  ca.pem  (certificate, mode 644)"
