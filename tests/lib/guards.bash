#!/usr/bin/env bash
# Skip guards for conditional test execution.

require_container() {
    for c in "$@"; do
        local state
        state=$(docker inspect --format '{{.State.Status}}' "$c" 2>/dev/null || echo "missing")
        [[ "$state" == "running" ]] || skip "Container $c not running ($state)"
        local health
        health=$(docker inspect --format '{{.State.Health.Status}}' "$c" 2>/dev/null || echo "none")
        [[ "$health" == "none" || "$health" == "healthy" ]] || skip "Container $c not healthy ($health)"
    done
}

require_network() {
    local host="$1" port="${2:-443}"
    timeout 3 bash -c "echo > /dev/tcp/$host/$port" 2>/dev/null || skip "$host:$port unreachable"
}

relax_security_level() {
    local ttl="${1:-120}"
    docker exec "$CTR_STATE" sh -c "
        REDISCLI_AUTH=\$(cat /run/secrets/valkey_mcp_admin_password) \
        valkey-cli --tls --cert /etc/valkey/tls/client.crt \
            --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
            --user mcp-admin --no-auth-warning \
            SET polis:config:security_level relaxed EX $ttl" 2>/dev/null || true
}

restore_security_level() {
    docker exec "$CTR_STATE" sh -c "
        REDISCLI_AUTH=\$(cat /run/secrets/valkey_mcp_admin_password) \
        valkey-cli --tls --cert /etc/valkey/tls/client.crt \
            --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
            --user mcp-admin --no-auth-warning \
            DEL polis:config:security_level" 2>/dev/null || true
}

reset_test_state() {
    restore_security_level
}

run_with_network_skip() {
    local label="$1"; shift
    run "$@"
    if [[ "$status" -ne 0 ]]; then
        case "$output" in
            *"Could not resolve"*|*"Connection timed out"*|\
            *"Network is unreachable"*|*"Connection refused"*)
                skip "${label} unreachable — network-dependent test"
                ;;
        esac
    fi
}
