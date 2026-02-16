#!/bin/bash
# =============================================================================
# polis-approve: Approve or deny blocked requests from the host
# =============================================================================
# Usage:
#   polis-approve approve <request_id>
#   polis-approve deny <request_id>
#   polis-approve list
# =============================================================================
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SECRETS_DIR="${PROJECT_ROOT}/secrets"

# Valkey connection via the state container
valkey_cmd() {
    local pass
    pass=$(cat "${SECRETS_DIR}/valkey_mcp_admin_password.txt")
    docker exec polis-state valkey-cli \
        --tls \
        --cert /etc/valkey/tls/client.crt \
        --key /etc/valkey/tls/client.key \
        --cacert /etc/valkey/tls/ca.crt \
        -a "$pass" --user mcp-admin --no-auth-warning \
        "$@"
}

ACTION="${1:-help}"
REQ_ID="${2:-}"

case "$ACTION" in
    approve)
        if [[ -z "$REQ_ID" ]]; then
            echo "Usage: polis-approve approve <request_id>"
            exit 1
        fi
        # Move from blocked to approved (5 min TTL)
        JSON=$(valkey_cmd GET "polis:blocked:${REQ_ID}")
        if [[ -z "$JSON" || "$JSON" == "(nil)" ]]; then
            echo "Request ${REQ_ID} not found (expired or already processed)."
            exit 1
        fi
        valkey_cmd SETEX "polis:approved:${REQ_ID}" 300 "$JSON" > /dev/null
        # Also set host-based approval so retries pass through DLP
        HOST=$(echo "$JSON" | grep -o '"destination":"[^"]*"' | cut -d'"' -f4)
        if [[ -n "$HOST" ]]; then
            valkey_cmd SETEX "polis:approved:host:${HOST}" 300 "1" > /dev/null
        fi
        valkey_cmd DEL "polis:blocked:${REQ_ID}" > /dev/null
        echo "✓ Approved ${REQ_ID} (destination: ${HOST:-unknown}, TTL: 300s)"
        ;;
    deny)
        if [[ -z "$REQ_ID" ]]; then
            echo "Usage: polis-approve deny <request_id>"
            exit 1
        fi
        valkey_cmd DEL "polis:blocked:${REQ_ID}" > /dev/null
        echo "✗ Denied ${REQ_ID}"
        ;;
    list)
        KEYS=$(valkey_cmd SCAN 0 MATCH 'polis:blocked:*' COUNT 100 | tail -n +2)
        if [[ -z "$KEYS" ]]; then
            echo "No pending requests."
            exit 0
        fi
        echo "Pending requests:"
        for key in $KEYS; do
            JSON=$(valkey_cmd GET "$key")
            REQ=$(echo "$JSON" | grep -o '"request_id":"[^"]*"' | cut -d'"' -f4)
            DEST=$(echo "$JSON" | grep -o '"destination":"[^"]*"' | cut -d'"' -f4)
            REASON=$(echo "$JSON" | grep -o '"reason":"[^"]*"' | cut -d'"' -f4)
            echo "  ${REQ}  →  ${DEST}  (${REASON})"
        done
        ;;
    *)
        echo "Usage: polis-approve <approve|deny|list> [request_id]"
        exit 1
        ;;
esac
