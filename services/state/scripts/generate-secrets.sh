#!/bin/bash
set -euo pipefail
umask 022

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
# Create individual password files (Docker secrets)
# =============================================================================

echo ""
echo "--- Creating Docker secret files ---"

echo -n "$PASS_HEALTHCHECK" > "${OUTPUT_DIR}/valkey_password.txt"
echo -n "$DLP_PASS" > "${OUTPUT_DIR}/valkey_dlp_password.txt"
echo -n "$PASS_MCP_AGENT" > "${OUTPUT_DIR}/valkey_mcp_agent_password.txt"
echo -n "$PASS_MCP_ADMIN" > "${OUTPUT_DIR}/valkey_mcp_admin_password.txt"
echo -n "$PASS_GOV_REQMOD" > "${OUTPUT_DIR}/valkey_reqmod_password.txt"
echo -n "$PASS_GOV_RESPMOD" > "${OUTPUT_DIR}/valkey_respmod_password.txt"
echo -n "$PASS_LOG_WRITER" > "${OUTPUT_DIR}/valkey_log_writer_password.txt"

echo "Created 7 password files for Docker secrets"

# =============================================================================
# Create valkey_users.acl
# ACL rules with SHA-256 hashed passwords for all users
# =============================================================================

echo ""
echo "--- Creating valkey_users.acl ---"

cat > "${OUTPUT_DIR}/valkey_users.acl" <<EOF
user default off
user governance-reqmod on #${HASH_GOV_REQMOD} ~polis:ott:* ~polis:blocked:* ~polis:approved:* ~polis:log:* -@all +get +set +setnx +exists +zadd
user governance-respmod on #${HASH_GOV_RESPMOD} ~polis:ott:* ~polis:blocked:* ~polis:approved:* ~polis:log:* -@all +get +del +setex +exists +zadd
user mcp-agent on #${HASH_MCP_AGENT} ~polis:blocked:* ~polis:approved:* -@all +GET +SET +SETEX +MGET +EXISTS +SCAN +PING +TTL (~polis:config:security_level -@all +GET +PING) (~polis:log:events -@all +ZADD +ZREMRANGEBYRANK +ZREVRANGE +ZCARD +PING)
user mcp-admin on #${HASH_MCP_ADMIN} ~polis:* +@all -@dangerous -FLUSHALL -FLUSHDB -DEBUG -CONFIG -SHUTDOWN
user log-writer on #${HASH_LOG_WRITER} ~polis:log:events -@all +ZADD +ZRANGEBYSCORE +ZCARD +PING
user healthcheck on #${HASH_HEALTHCHECK} -@all +PING +INFO
user dlp-reader on #${HASH_DLP} ~polis:config:security_level -@all +GET +PING
EOF

echo "valkey_users.acl created"

# =============================================================================
# Write .env file (non-secret configuration only)
# Passwords are now read from Docker secrets, not environment variables
# =============================================================================

echo ""
echo "--- Updating .env file ---"

if [[ -f "$ENV_FILE" ]]; then
    # Remove any existing VALKEY_ password vars (migration cleanup)
    sed -i '/^VALKEY_MCP_AGENT_PASS=/d' "$ENV_FILE"
    sed -i '/^VALKEY_MCP_ADMIN_PASS=/d' "$ENV_FILE"
    sed -i '/^VALKEY_REQMOD_PASS=/d' "$ENV_FILE"
    sed -i '/^VALKEY_RESPMOD_PASS=/d' "$ENV_FILE"
fi

echo ".env cleaned (passwords removed, now using Docker secrets)"

echo ""
echo "=== Secrets generation complete ==="
echo "Files created in: ${OUTPUT_DIR}"
echo "  valkey_password.txt              (healthcheck)"
echo "  valkey_dlp_password.txt          (DLP reader)"
echo "  valkey_mcp_agent_password.txt    (MCP agent)"
echo "  valkey_mcp_admin_password.txt    (MCP admin)"
echo "  valkey_reqmod_password.txt       (ICAP REQMOD)"
echo "  valkey_respmod_password.txt      (ICAP RESPMOD)"
echo "  valkey_log_writer_password.txt   (log writer)"
echo "  valkey_users.acl                 (ACL with hashes)"
echo ""
echo "✅ Passwords stored as Docker secrets (mounted at /run/secrets/)"
echo "✅ .env cleaned (no plaintext passwords)"
echo ""
echo "⚠️  SECURITY NOTE:"
echo "  - All passwords now read from Docker secrets"
echo "  - .env contains only non-secret configuration"
echo "  - For production, use external secret manager (Vault, AWS Secrets Manager)"
echo "  - All files in secrets/ are gitignored"
