#!/bin/bash
set -euo pipefail

# =============================================================================
# Toolbox TLS Certificate Generator
# Generates server certificate signed by Polis internal CA
# =============================================================================

# Output directory (default: ./certs/toolbox)
OUTPUT_DIR="${1:-./certs/toolbox}"

# CA directory (default: ./certs/ca)
CA_DIR="${2:-./certs/ca}"

DAYS=365

echo "=== Toolbox TLS Certificate Generator ==="
echo "Output directory: ${OUTPUT_DIR}"
echo "CA directory: ${CA_DIR}"

# Check CA exists
if [[ ! -f "${CA_DIR}/ca.pem" ]] || [[ ! -f "${CA_DIR}/ca.key" ]]; then
    echo "ERROR: CA certificate not found at ${CA_DIR}"
    echo "Run 'polis init' or generate CA first."
    exit 1
fi

# Skip if certs already exist
if [[ -f "${OUTPUT_DIR}/toolbox.pem" ]] && [[ -f "${OUTPUT_DIR}/toolbox.key" ]]; then
    echo "Toolbox certificates already exist. Skipping."
    exit 0
fi

mkdir -p "${OUTPUT_DIR}"

echo ""
echo "--- Generating server certificate ---"

# Generate key
openssl genrsa -out "${OUTPUT_DIR}/toolbox.key" 2048 2>/dev/null

# Generate CSR
openssl req -new \
    -key "${OUTPUT_DIR}/toolbox.key" \
    -out "${OUTPUT_DIR}/toolbox.csr" \
    -subj "/CN=toolbox/O=polis" 2>/dev/null

# Create extensions file for SANs
cat > "${OUTPUT_DIR}/toolbox.ext" << 'EOF'
authorityKeyIdentifier=keyid,issuer
basicConstraints=CA:FALSE
keyUsage = digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = @alt_names

[alt_names]
DNS.1 = toolbox
DNS.2 = polis-toolbox
DNS.3 = localhost
IP.1 = 10.10.1.20
IP.2 = 10.30.1.20
IP.3 = 127.0.0.1
EOF

# Sign with CA
openssl x509 -req \
    -in "${OUTPUT_DIR}/toolbox.csr" \
    -CA "${CA_DIR}/ca.pem" \
    -CAkey "${CA_DIR}/ca.key" \
    -CAcreateserial \
    -out "${OUTPUT_DIR}/toolbox.pem" \
    -days "${DAYS}" \
    -sha256 \
    -extfile "${OUTPUT_DIR}/toolbox.ext" 2>/dev/null

# Cleanup temp files
rm -f "${OUTPUT_DIR}/toolbox.csr" "${OUTPUT_DIR}/toolbox.ext" "${CA_DIR}/ca.srl"

# Set permissions: key=600 (owner only), cert=644 (public)
# polis.sh will sudo chown 65532 after generation so containers can read the key
chmod 600 "${OUTPUT_DIR}/toolbox.key"
chmod 644 "${OUTPUT_DIR}/toolbox.pem"

echo ""
echo "=== Certificate generation complete ==="
echo "Files created in: ${OUTPUT_DIR}"
echo "  toolbox.key  (private key)"
echo "  toolbox.pem  (certificate)"
