#!/bin/bash
set -euo pipefail

# =============================================================================
# Valkey Secrets Generator
# Generates passwords, ACL configuration, and credential references
# for all Valkey service users.
# =============================================================================

# Output directory (default: ./secrets)
OUTPUT_DIR="${1:-./secrets}"
# Project root for .env file (default: parent of output dir)
PROJECT_ROOT="${2:-$(cd "$(dirname "${OUTPUT_DIR}")" && pwd)}"
ENV_FILE="${PROJECT_ROOT}/.env"

echo "=== Valkey Secrets Generator ==="
echo "Output directory: ${OUTPUT_DIR}"

# Create output directory if it doesn't exist
mkdir -p "${OUTPUT_DIR}"

# =============================================================================
# Password Generation
# Generate unique 32-character alphanumeric passwords for each user
# =============================================================================

echo ""
echo "--- Generating passwords ---"

generate_password() {
    openssl rand -base64 32 | tr -d '/+=' | head -c 32
}

PASS_MCP_AGENT="$(generate_password)"
PASS_MCP_ADMIN="$(generate_password)"
PASS_LOG_WRITER="$(generate_password)"
PASS_HEALTHCHECK="$(generate_password)"
PASS_GOV_REQMOD="$(generate_password)"
PASS_GOV_RESPMOD="$(generate_password)"
DLP_PASS="$(generate_password)"

echo "Generated 7 unique passwords (32 characters each)."

# =============================================================================
# SHA-256 Hash Generation
# Hash each password for use in the ACL file
# =============================================================================

echo ""
echo "--- Computing SHA-256 hashes ---"

hash_password() {
    echo -n "$1" | sha256sum | awk '{print $1}'
}

HASH_MCP_AGENT="$(hash_password "${PASS_MCP_AGENT}")"
HASH_MCP_ADMIN="$(hash_password "${PASS_MCP_ADMIN}")"
HASH_LOG_WRITER="$(hash_password "${PASS_LOG_WRITER}")"
HASH_HEALTHCHECK="$(hash_password "${PASS_HEALTHCHECK}")"
HASH_GOV_REQMOD="$(hash_password "${PASS_GOV_REQMOD}")"
HASH_GOV_RESPMOD="$(hash_password "${PASS_GOV_RESPMOD}")"
HASH_DLP="$(hash_password "${DLP_PASS}")"

echo "SHA-256 hashes computed for all users."

# =============================================================================
# Create valkey_password.txt
# Contains the healthcheck user password (used by Docker health check)
# =============================================================================

echo ""
echo "--- Creating valkey_password.txt ---"

cat > "${OUTPUT_DIR}/valkey_password.txt" <<EOF
${PASS_HEALTHCHECK}
EOF

echo "valkey_password.txt created (healthcheck password)."

# =============================================================================
# Create valkey_users.acl
# ACL rules with SHA-256 hashed passwords for all users
# Includes: governance ICAP users, MCP users, utility users
# =============================================================================

echo ""
echo "--- Creating valkey_users.acl ---"

cat > "${OUTPUT_DIR}/valkey_users.acl" <<EOF
user default off
user governance-reqmod on #${HASH_GOV_REQMOD} ~polis:ott:* ~polis:blocked:* ~polis:log:* -@all +get +set +setnx +exists +zadd
user governance-respmod on #${HASH_GOV_RESPMOD} ~polis:ott:* ~polis:blocked:* ~polis:approved:* ~polis:log:* -@all +get +del +setex +exists +zadd
user mcp-agent on #${HASH_MCP_AGENT} ~polis:blocked:* ~polis:approved:* -@all +GET +SET +SETEX +MGET +EXISTS +SCAN +PING +TTL (~polis:config:security_level -@all +GET +PING) (~polis:log:events -@all +RPUSH +LTRIM +LRANGE +PING)
user mcp-admin on #${HASH_MCP_ADMIN} ~polis:* +@all -@dangerous -FLUSHALL -FLUSHDB -DEBUG -CONFIG -SHUTDOWN
user log-writer on #${HASH_LOG_WRITER} ~polis:log:events -@all +ZADD +ZRANGEBYSCORE +ZCARD +PING
user healthcheck on #${HASH_HEALTHCHECK} -@all +PING +INFO
user dlp-reader on #${HASH_DLP} ~polis:config:security_level -@all +GET +PING
EOF

echo "valkey_users.acl created (8 user entries)."

# =============================================================================
# Create valkey_dlp_password.txt
# Contains the dlp-reader user password (used by DLP module Docker secret)
# =============================================================================

echo ""
echo "--- Creating valkey_dlp_password.txt ---"

echo -n "$DLP_PASS" > "${OUTPUT_DIR}/valkey_dlp_password.txt"

echo "valkey_dlp_password.txt created (dlp-reader password)."

# =============================================================================
# Write required env vars to .env
# Docker-compose and tests need these passwords
# =============================================================================

echo ""
echo "--- Updating .env file ---"

if [[ -f "$ENV_FILE" ]]; then
    # Remove any existing VALKEY_ vars to avoid duplicates
    sed -i '/^VALKEY_MCP_AGENT_PASS=/d' "$ENV_FILE"
    sed -i '/^VALKEY_MCP_ADMIN_PASS=/d' "$ENV_FILE"
    sed -i '/^VALKEY_REQMOD_PASS=/d' "$ENV_FILE"
    sed -i '/^VALKEY_RESPMOD_PASS=/d' "$ENV_FILE"
fi

# Append all passwords needed by docker-compose and tests
echo "VALKEY_MCP_AGENT_PASS=${PASS_MCP_AGENT}" >> "$ENV_FILE"
echo "VALKEY_MCP_ADMIN_PASS=${PASS_MCP_ADMIN}" >> "$ENV_FILE"
echo "VALKEY_REQMOD_PASS=${PASS_GOV_REQMOD}" >> "$ENV_FILE"
echo "VALKEY_RESPMOD_PASS=${PASS_GOV_RESPMOD}" >> "$ENV_FILE"

echo "Valkey passwords written to ${ENV_FILE}"

# =============================================================================
# Set File Permissions
# =============================================================================

echo ""
echo "--- Setting file permissions ---"

chmod 600 "${OUTPUT_DIR}/valkey_password.txt"
chmod 644 "${OUTPUT_DIR}/valkey_users.acl"
chmod 644 "${OUTPUT_DIR}/valkey_dlp_password.txt"

echo "Permissions set: valkey_users.acl=644, valkey_dlp_password.txt=644 (readable by services), valkey_password.txt=600"

echo ""
echo "=== Secrets generation complete ==="
echo "Files created in: ${OUTPUT_DIR}"
echo "  valkey_password.txt      (healthcheck password)"
echo "  valkey_users.acl         (ACL with hashed passwords)"
echo "  valkey_dlp_password.txt  (dlp-reader password)"
echo ""
echo "Passwords written to: ${ENV_FILE}"
