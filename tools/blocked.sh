#!/usr/bin/env bash
# blocked.sh — HITL approval workflow for polis blocked requests
# Usage: blocked.sh <pending|approve|deny|check|log> [request_id]
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SECRETS_DIR="${PROJECT_ROOT}/secrets"

_valkey_cmd() {
    local pass
    pass=$(cat "${SECRETS_DIR}/valkey_mcp_admin_password.txt" 2>/dev/null) || {
        echo "[ERROR] Valkey admin password not found. Run 'just setup' first." >&2
        exit 1
    }
    docker exec polis-state valkey-cli \
        --tls \
        --cert /etc/valkey/tls/client.crt \
        --key /etc/valkey/tls/client.key \
        --cacert /etc/valkey/tls/ca.crt \
        -a "$pass" --user mcp-admin --no-auth-warning \
        "$@"
    return $?
}

case "${1:-pending}" in
    pending|list)
        KEYS=$(_valkey_cmd SCAN 0 MATCH 'polis:blocked:*' COUNT 100 | tail -n +2)
        if [[ -z "$KEYS" ]]; then
            echo "No pending requests."
            exit 0
        fi
        echo "Pending blocked requests:"
        for key in $KEYS; do
            JSON=$(_valkey_cmd GET "$key")
            REQ=$(echo "$JSON" | grep -o '"request_id":"[^"]*"' | cut -d'"' -f4)
            DEST=$(echo "$JSON" | grep -o '"destination":"[^"]*"' | cut -d'"' -f4)
            REASON=$(echo "$JSON" | grep -o '"reason":"[^"]*"' | cut -d'"' -f4)
            echo "  ${REQ}  →  ${DEST}  (${REASON})"
        done
        ;;
    approve)
        REQ_ID="${2:?Usage: blocked.sh approve <request_id>}"
        JSON=$(_valkey_cmd GET "polis:blocked:${REQ_ID}")
        if [[ -z "$JSON" || "$JSON" == "(nil)" ]]; then
            echo "[ERROR] Request ${REQ_ID} not found (expired or already processed)." >&2
            exit 1
        fi
        _valkey_cmd SETEX "polis:approved:${REQ_ID}" 300 "$JSON" > /dev/null
        HOST=$(echo "$JSON" | grep -o '"destination":"[^"]*"' | cut -d'"' -f4)
        if [[ -n "$HOST" ]]; then
            _valkey_cmd SETEX "polis:approved:host:${HOST}" 300 "1" > /dev/null
        fi
        _valkey_cmd DEL "polis:blocked:${REQ_ID}" > /dev/null
        echo "[OK] Approved ${REQ_ID} (destination: ${HOST:-unknown}, TTL: 300s)"
        ;;
    deny)
        REQ_ID="${2:?Usage: blocked.sh deny <request_id>}"
        _valkey_cmd DEL "polis:blocked:${REQ_ID}" > /dev/null
        echo "[OK] Denied ${REQ_ID}"
        ;;
    check)
        REQ_ID="${2:?Usage: blocked.sh check <request_id>}"
        if _valkey_cmd GET "polis:blocked:${REQ_ID}" | grep -q request_id; then
            echo "Status: pending"
        elif _valkey_cmd GET "polis:approved:${REQ_ID}" | grep -q request_id; then
            echo "Status: approved"
        else
            echo "Status: not found (expired or denied)"
        fi
        ;;
    log)
        docker exec polis-workspace /opt/agents/openclaw/scripts/polis-mcp-call.sh \
            get_security_log "{}" 2>/dev/null || \
            echo "[WARN] Could not retrieve security log. Is the workspace running?" >&2
        ;;
    *)
        echo "Usage: polis blocked <pending|approve|deny|check|log> [request_id]"
        echo ""
        echo "Commands:"
        echo "  pending              List pending blocked requests (default)"
        echo "  approve <id>         Approve a blocked request (5 min TTL)"
        echo "  deny <id>            Deny a blocked request"
        echo "  check <id>           Check status of a request"
        echo "  log                  Show recent security events"
        exit 1
        ;;
esac
