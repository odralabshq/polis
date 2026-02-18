#!/bin/bash
set -euo pipefail

# Prevent MSYS/Git-Bash path conversion on Windows
export MSYS_NO_PATHCONV=1

# =============================================================================
# Valkey TLS Certificate Generator
# Generates CA, server, and client certificates for Valkey mTLS
# =============================================================================

# Output directory (default: ./certs/valkey)
OUTPUT_DIR="${1:-./certs/valkey}"

# Certificate parameters
CA_SUBJECT="/CN=polis-state-CA/O=OdraLabs"
SERVER_SUBJECT="/CN=state/O=OdraLabs"
CLIENT_SUBJECT="/CN=state-client/O=OdraLabs"
DAYS=365

echo "=== Valkey TLS Certificate Generator ==="
echo "Output directory: ${OUTPUT_DIR}"

# Create output directory if it doesn't exist
mkdir -p "${OUTPUT_DIR}"

# ---- CA Certificate ----
echo ""
echo "--- Generating CA certificate ---"
openssl genrsa -out "${OUTPUT_DIR}/ca.key" 4096 2>/dev/null

# Create CA extension file
CA_EXT_FILE="${OUTPUT_DIR}/ca.ext"
cat > "${CA_EXT_FILE}" << EOF
basicConstraints = critical, CA:TRUE
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
EOF

openssl req -new -x509 \
    -key "${OUTPUT_DIR}/ca.key" \
    -out "${OUTPUT_DIR}/ca.crt" \
    -days "${DAYS}" \
    -sha256 \
    -subj "${CA_SUBJECT}" \
    -extensions v3_ca \
    -config <(cat /etc/ssl/openssl.cnf <(printf "\n[v3_ca]\n") "${CA_EXT_FILE}")

rm -f "${CA_EXT_FILE}"
echo "CA certificate created."

# ---- Server Certificate ----
echo ""
echo "--- Generating server certificate ---"
openssl genrsa -out "${OUTPUT_DIR}/server.key" 2048 2>/dev/null

# Create extension file for server cert
EXT_FILE="${OUTPUT_DIR}/server.ext"
cat > "${EXT_FILE}" << EOF
basicConstraints = CA:FALSE
keyUsage = critical, digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = DNS:state,DNS:valkey,DNS:localhost,IP:127.0.0.1
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid,issuer
EOF

openssl req -new \
    -key "${OUTPUT_DIR}/server.key" \
    -out "${OUTPUT_DIR}/server.csr" \
    -sha256 \
    -subj "${SERVER_SUBJECT}"

openssl x509 -req \
    -in "${OUTPUT_DIR}/server.csr" \
    -CA "${OUTPUT_DIR}/ca.crt" \
    -CAkey "${OUTPUT_DIR}/ca.key" \
    -CAcreateserial \
    -out "${OUTPUT_DIR}/server.crt" \
    -days "${DAYS}" \
    -sha256 \
    -extfile "${EXT_FILE}"

# Remove server CSR and ext file
rm -f "${OUTPUT_DIR}/server.csr" "${EXT_FILE}"

echo "Server certificate created."

# ---- Client Certificate ----
echo ""
echo "--- Generating client certificate ---"
openssl genrsa -out "${OUTPUT_DIR}/client.key" 2048 2>/dev/null

# Create extension file for client cert
CLIENT_EXT_FILE="${OUTPUT_DIR}/client.ext"
cat > "${CLIENT_EXT_FILE}" << EOF
basicConstraints = CA:FALSE
keyUsage = critical, digitalSignature
extendedKeyUsage = clientAuth
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid,issuer
EOF

openssl req -new \
    -key "${OUTPUT_DIR}/client.key" \
    -out "${OUTPUT_DIR}/client.csr" \
    -sha256 \
    -subj "${CLIENT_SUBJECT}"

openssl x509 -req \
    -in "${OUTPUT_DIR}/client.csr" \
    -CA "${OUTPUT_DIR}/ca.crt" \
    -CAkey "${OUTPUT_DIR}/ca.key" \
    -CAcreateserial \
    -out "${OUTPUT_DIR}/client.crt" \
    -days "${DAYS}" \
    -sha256 \
    -extfile "${CLIENT_EXT_FILE}"

# Remove client CSR and ext file
rm -f "${OUTPUT_DIR}/client.csr" "${CLIENT_EXT_FILE}"

echo "Client certificate created."

# ---- Clean up CA serial file ----
rm -f "${OUTPUT_DIR}/ca.srl"

# ---- Set file permissions ----
echo ""
echo "--- Setting file permissions ---"

# Private keys: 600 (owner read/write only)
# polis.sh will sudo chown 65532 after generation so containers can read them
chmod 600 "${OUTPUT_DIR}/ca.key"
chmod 600 "${OUTPUT_DIR}/server.key"
chmod 600 "${OUTPUT_DIR}/client.key"

# Certificates: 644 (owner read/write, group/other read)
chmod 644 "${OUTPUT_DIR}/ca.crt"
chmod 644 "${OUTPUT_DIR}/server.crt"
chmod 644 "${OUTPUT_DIR}/client.crt"

echo "Permissions set: keys=600, certs=644"

echo ""
echo "=== Certificate generation complete ==="
echo "Files created in: ${OUTPUT_DIR}"
echo "  ca.key, ca.crt       (CA)"
echo "  server.key, server.crt (Server)"
echo "  client.key, client.crt (Client)"
