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

mkdir -p "${OUTPUT_DIR}"

generate_password() {
    openssl rand -base64 32 | tr -d '/+=' | head -c 32
}

ensure_password_file() {
    local path="$1"
    ensure_output_file_path "${path}"
    if [[ -s "${path}" ]]; then
        tr -d '[:space:]' < "${path}"
    else
        local password
        password="$(generate_password)"
        printf '%s' "${password}" > "${path}"
        chmod 600 "${path}" 2>/dev/null || true
        printf '%s' "${password}"
    fi
}

ensure_output_file_path() {
    local path="$1"
    if [[ -d "${path}" ]]; then
        if find "${path}" -mindepth 1 -print -quit | grep -q .; then
            echo "Refusing to replace non-empty directory at ${path}" >&2
            return 1
        fi
        rmdir "${path}"
    fi
}

hash_password() {
    echo -n "$1" | sha256sum | awk '{print $1}'
}

echo ""
echo "--- Ensuring password files ---"

PASS_HEALTHCHECK="$(ensure_password_file "${OUTPUT_DIR}/valkey_password.txt")"
DLP_PASS="$(ensure_password_file "${OUTPUT_DIR}/valkey_dlp_password.txt")"
PASS_MCP_AGENT="$(ensure_password_file "${OUTPUT_DIR}/valkey_mcp_agent_password.txt")"
PASS_MCP_ADMIN="$(ensure_password_file "${OUTPUT_DIR}/valkey_mcp_admin_password.txt")"
PASS_GOV_REQMOD="$(ensure_password_file "${OUTPUT_DIR}/valkey_reqmod_password.txt")"
PASS_GOV_RESPMOD="$(ensure_password_file "${OUTPUT_DIR}/valkey_respmod_password.txt")"
PASS_LOG_WRITER="$(ensure_password_file "${OUTPUT_DIR}/valkey_log_writer_password.txt")"
PASS_CP_SERVER="$(ensure_password_file "${OUTPUT_DIR}/valkey_cp_server_password.txt")"

echo "Password files are present for all Valkey users."

echo ""
echo "--- Computing SHA-256 hashes ---"

HASH_MCP_AGENT="$(hash_password "${PASS_MCP_AGENT}")"
HASH_MCP_ADMIN="$(hash_password "${PASS_MCP_ADMIN}")"
HASH_LOG_WRITER="$(hash_password "${PASS_LOG_WRITER}")"
HASH_HEALTHCHECK="$(hash_password "${PASS_HEALTHCHECK}")"
HASH_GOV_REQMOD="$(hash_password "${PASS_GOV_REQMOD}")"
HASH_GOV_RESPMOD="$(hash_password "${PASS_GOV_RESPMOD}")"
HASH_DLP="$(hash_password "${DLP_PASS}")"
HASH_CP_SERVER="$(hash_password "${PASS_CP_SERVER}")"

echo "SHA-256 hashes computed for all users."

echo ""
echo "--- Writing valkey_users.acl ---"

ensure_output_file_path "${OUTPUT_DIR}/valkey_users.acl"

cat > "${OUTPUT_DIR}/valkey_users.acl" <<EOF
user default off
user governance-reqmod on #${HASH_GOV_REQMOD} ~polis:ott:* ~polis:blocked:* ~polis:approved:* ~polis:log:* -@all +get +set +setex +setnx +exists +zadd
user governance-respmod on #${HASH_GOV_RESPMOD} ~polis:ott:* ~polis:blocked:* ~polis:approved:* ~polis:log:* -@all +get +del +setex +exists +zadd
user mcp-agent on #${HASH_MCP_AGENT} ~polis:blocked:* ~polis:approved:* -@all +GET +SET +SETEX +MGET +EXISTS +SCAN +PING +TTL (~polis:config:security_level -@all +GET +PING) (~polis:log:events -@all +ZADD +ZREMRANGEBYRANK +ZREVRANGE +ZCARD +PING)
user cp-server on #${HASH_CP_SERVER} ~polis:blocked:* ~polis:approved:* ~polis:config:security_level ~polis:config:auto_approve:* ~polis:log:events -@all +GET +SET +SETEX +DEL +MGET +EXISTS +SCAN +PING +ZADD +ZREVRANGE +ZCARD +ZREMRANGEBYRANK
user mcp-admin on #${HASH_MCP_ADMIN} ~polis:* +@all -@dangerous -FLUSHALL -FLUSHDB -DEBUG -CONFIG -SHUTDOWN
user log-writer on #${HASH_LOG_WRITER} ~polis:log:events -@all +ZADD +ZRANGEBYSCORE +ZCARD +PING
user healthcheck on #${HASH_HEALTHCHECK} -@all +PING +INFO
user dlp-reader on #${HASH_DLP} ~polis:config:security_level -@all +GET +PING
EOF

echo "valkey_users.acl written"

echo ""
echo "--- Updating .env file ---"

if [[ -f "${ENV_FILE}" ]]; then
    sed -i '/^VALKEY_MCP_AGENT_PASS=/d' "${ENV_FILE}"
    sed -i '/^VALKEY_MCP_ADMIN_PASS=/d' "${ENV_FILE}"
    sed -i '/^VALKEY_REQMOD_PASS=/d' "${ENV_FILE}"
    sed -i '/^VALKEY_RESPMOD_PASS=/d' "${ENV_FILE}"
fi

echo ".env cleaned (passwords removed, now using Docker secrets)"

echo ""
echo "=== Secrets generation complete ==="
echo "Files ensured in: ${OUTPUT_DIR}"
echo "  valkey_password.txt              (healthcheck)"
echo "  valkey_dlp_password.txt          (DLP reader)"
echo "  valkey_mcp_agent_password.txt    (MCP agent)"
echo "  valkey_mcp_admin_password.txt    (MCP admin)"
echo "  valkey_reqmod_password.txt       (ICAP REQMOD)"
echo "  valkey_respmod_password.txt      (ICAP RESPMOD)"
echo "  valkey_log_writer_password.txt   (log writer)"
echo "  valkey_cp_server_password.txt    (control plane)"
echo "  valkey_users.acl                 (ACL with hashes)"
