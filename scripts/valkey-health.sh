#!/bin/bash
set -euo pipefail

# =============================================================================
# Valkey Health Check Script
# Verifies Valkey connectivity, memory pressure, and AOF persistence status.
# Connects via TLS with client certificates and authenticates using
# REDISCLI_AUTH (password never visible in ps output).
#
# Exit codes:
#   0 - All checks pass (outputs "OK")
#   1 - Any check fails (outputs "CRITICAL: ..." or "WARNING: ...")
#
# Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 6.7
# =============================================================================

# =============================================================================
# Environment Variable Defaults
# =============================================================================

VALKEY_HOST="${VALKEY_HOST:-valkey}"
VALKEY_PORT="${VALKEY_PORT:-6379}"
VALKEY_PASSWORD_FILE="${VALKEY_PASSWORD_FILE:-/run/secrets/valkey_password}"
VALKEY_TLS_CERT="${VALKEY_TLS_CERT:-/etc/valkey/tls/client.crt}"
VALKEY_TLS_KEY="${VALKEY_TLS_KEY:-/etc/valkey/tls/client.key}"
VALKEY_TLS_CA="${VALKEY_TLS_CA:-/etc/valkey/tls/ca.crt}"
MEMORY_WARN_PERCENT="${MEMORY_WARN_PERCENT:-80}"

# =============================================================================
# Input Validation
# Requirement 6.6: Exit 1 with CRITICAL if host or port are invalid
# =============================================================================

# Validate VALKEY_HOST: must match ^[a-zA-Z0-9._-]+$
if ! echo "${VALKEY_HOST}" | grep -qE '^[a-zA-Z0-9._-]+$'; then
    echo "CRITICAL: Invalid VALKEY_HOST '${VALKEY_HOST}'"
    exit 1
fi

# Validate VALKEY_PORT: must be numeric and in range 1-65535
if ! echo "${VALKEY_PORT}" | grep -qE '^[0-9]+$'; then
    echo "CRITICAL: Invalid VALKEY_PORT '${VALKEY_PORT}' (not numeric)"
    exit 1
fi
if [ "${VALKEY_PORT}" -lt 1 ] || [ "${VALKEY_PORT}" -gt 65535 ]; then
    echo "CRITICAL: Invalid VALKEY_PORT '${VALKEY_PORT}' (out of range 1-65535)"
    exit 1
fi

# =============================================================================
# Password Loading
# Requirement 6.5: Read password from file, export as REDISCLI_AUTH
# (not visible in ps aux output)
# =============================================================================

if [ ! -f "${VALKEY_PASSWORD_FILE}" ]; then
    echo "CRITICAL: Password file not found: ${VALKEY_PASSWORD_FILE}"
    exit 1
fi

REDISCLI_AUTH="$(cat "${VALKEY_PASSWORD_FILE}" | tr -d '[:space:]')"
export REDISCLI_AUTH

# =============================================================================
# Build valkey-cli command arguments
# =============================================================================

CLI_ARGS=(
    --tls
    --cert "${VALKEY_TLS_CERT}"
    --key "${VALKEY_TLS_KEY}"
    --cacert "${VALKEY_TLS_CA}"
    -h "${VALKEY_HOST}"
    -p "${VALKEY_PORT}"
)

# =============================================================================
# Connectivity Check
# Requirement 6.2: Verify Valkey responds to PING with PONG
# =============================================================================

PING_RESULT="$(valkey-cli "${CLI_ARGS[@]}" ping 2>&1)" || true

if [ "${PING_RESULT}" != "PONG" ]; then
    echo "CRITICAL: Valkey not responding (expected PONG, got '${PING_RESULT}')"
    exit 1
fi

# =============================================================================
# Memory Pressure Check
# Requirement 6.3: Warn if memory usage >= configurable threshold
# =============================================================================

MEMORY_INFO="$(valkey-cli "${CLI_ARGS[@]}" info memory 2>&1)" || true

USED_MEMORY="$(echo "${MEMORY_INFO}" \
    | grep '^used_memory:' \
    | cut -d: -f2 \
    | tr -d '[:space:]')"

MAXMEMORY="$(echo "${MEMORY_INFO}" \
    | grep '^maxmemory:' \
    | cut -d: -f2 \
    | tr -d '[:space:]')"

# Only check memory pressure if maxmemory is configured (non-zero)
if [ -n "${MAXMEMORY}" ] && [ "${MAXMEMORY}" -gt 0 ] 2>/dev/null; then
    if [ -n "${USED_MEMORY}" ]; then
        # Calculate percentage: (used * 100) / max
        MEMORY_PERCENT=$(( (USED_MEMORY * 100) / MAXMEMORY ))
        if [ "${MEMORY_PERCENT}" -ge "${MEMORY_WARN_PERCENT}" ]; then
            echo "WARNING: Memory usage at ${MEMORY_PERCENT}% (threshold: ${MEMORY_WARN_PERCENT}%)"
            exit 1
        fi
    fi
fi

# =============================================================================
# AOF Persistence Check
# Requirement 6.4: Verify AOF is enabled, exit 1 if disabled
# =============================================================================

PERSIST_INFO="$(valkey-cli "${CLI_ARGS[@]}" info persistence 2>&1)" || true

AOF_ENABLED="$(echo "${PERSIST_INFO}" \
    | grep '^aof_enabled:' \
    | cut -d: -f2 \
    | tr -d '[:space:]')"

if [ "${AOF_ENABLED}" != "1" ]; then
    echo "CRITICAL: AOF persistence disabled"
    exit 1
fi

# =============================================================================
# All checks passed
# Requirement 6.7: Exit 0 with "OK"
# =============================================================================

echo "OK"
exit 0
