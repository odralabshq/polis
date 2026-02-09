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
CA_SUBJECT="/CN=Molis-Valkey-CA/O=OdraLabs"
SERVER_SUBJECT="/CN=valkey/O=OdraLabs"
CLIENT_SUBJECT="/CN=valkey-client/O=OdraLabs"
DAYS=365

echo "=== Valkey TLS Certificate Generator ==="
echo "Output directory: ${OUTPUT_DIR}"

# Create output directory if it doesn't exist
mkdir -p "${OUTPUT_DIR}"

# ---- CA Certificate ----
echo ""
echo "--- Generating CA certificate ---"
openssl genrsa -out "${OUTPUT_DIR}/ca.key" 4096 2>/dev/null

openssl req -new -x509 \
    -key "${OUTPUT_DIR}/ca.key" \
    -out "${OUTPUT_DIR}/ca.crt" \
    -days "${DAYS}" \
    -sha256 \
    -subj "${CA_SUBJECT}"

echo "CA certificate created."

# ---- Server Certificate ----
echo ""
echo "--- Generating server certificate ---"
openssl genrsa -out "${OUTPUT_DIR}/server.key" 2048 2>/dev/null

# Create extension file for SANs
EXT_FILE="${OUTPUT_DIR}/server.ext"
echo "subjectAltName = DNS:valkey,DNS:localhost,IP:127.0.0.1" > "${EXT_FILE}"

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
    -sha256

# Remove client CSR
rm -f "${OUTPUT_DIR}/client.csr"

echo "Client certificate created."

# ---- Clean up CA serial file ----
rm -f "${OUTPUT_DIR}/ca.srl"

# ---- Set file permissions ----
echo ""
echo "--- Setting file permissions ---"

# Private keys: 600 (owner read/write only)
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
