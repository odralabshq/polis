#!/usr/bin/env bash
# Skip guards for conditional test execution.

require_container() {
    for c in "$@"; do
        local state
        state=$(docker inspect --format '{{.State.Status}}' "$c" 2>/dev/null || echo "missing")
        [[ "$state" == "running" ]] || skip "Container $c not running ($state)"
        local health
        health=$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$c" 2>/dev/null || echo "none")
        [[ "$health" == "none" || "$health" == "healthy" ]] || skip "Container $c not healthy ($health)"
    done
    return 0
}

# Guard for init containers — they exit after running, so check exited with status 0
require_init_container() {
    for c in "$@"; do
        local state exit_code
        state=$(docker inspect --format '{{.State.Status}}' "$c" 2>/dev/null || echo "missing")
        [[ "$state" == "exited" ]] || skip "Init container $c not found (state: $state)"
        exit_code=$(docker inspect --format '{{.State.ExitCode}}' "$c" 2>/dev/null || echo "1")
        [[ "$exit_code" == "0" ]] || skip "Init container $c exited with code $exit_code"
    done
}

require_agents_mounted() {
    require_container "$CTR_WORKSPACE"
    docker exec "$CTR_WORKSPACE" test -f /tmp/agents/openclaw/scripts/polis-toolbox-call.sh 2>/dev/null \
        || skip "Agent scripts not mounted in workspace (compose.override.yaml not generated)"
}

require_network() {
    local host="$1" port="${2:-443}"
    timeout 3 bash -c "echo > /dev/tcp/$host/$port" 2>/dev/null || skip "$host:$port unreachable"
}

# Pre-approve a host in Valkey so HITL does not block it during tests.
# Usage: approve_host <host> [ttl_seconds]
approve_host() {
    local host="$1" ttl="${2:-600}"
    # Retry: state container may still be initializing in CI
    for _attempt in 1 2 3 4 5; do
        if docker exec "$CTR_STATE" sh -c "
            REDISCLI_AUTH=\$(cat /run/secrets/valkey_mcp_admin_password) \
            valkey-cli --tls --cert /etc/valkey/tls/client.crt \
                --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
                --user mcp-admin --no-auth-warning \
                SETEX 'polis:approved:host:${host}' ${ttl} '1'" 2>/dev/null; then
            return 0
        fi
        sleep 2
    done
    echo "Warning: approve_host ${host} failed after 5 attempts" >&2
}

relax_security_level() {
    local ttl="${1:-600}"
    # Retry setting security level (state container may still be initializing)
    for _attempt in 1 2 3 4 5; do
        if docker exec "$CTR_STATE" sh -c "
            REDISCLI_AUTH=\$(cat /run/secrets/valkey_mcp_admin_password) \
            valkey-cli --tls --cert /etc/valkey/tls/client.crt \
                --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
                --user mcp-admin --no-auth-warning \
                SET polis:config:security_level relaxed EX $ttl" 2>/dev/null; then
            # Warmup: wait for proxy to stabilise after security level change
            # HTTP is intentional - testing TPROXY interception of plain HTTP traffic
            for _i in 1 2 3; do
                docker exec "$CTR_WORKSPACE" curl -sf -o /dev/null --connect-timeout 5 \
                    --proxy "http://${IP_GATE_INT}:8080" "http://${HTTPBIN_HOST}/get" 2>/dev/null && return  # NOSONAR
                sleep 1
            done
            return 0
        fi
        sleep 2
    done
    echo "Warning: Failed to set security_level after 5 attempts" >&2
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
    # Wrap command in timeout to prevent SIGINT from propagating to BATS
    # timeout returns 124 on timeout, preserving original exit code otherwise
    run timeout --signal=KILL 30s "$@"
    if [[ "$status" -ne 0 ]]; then
        # 124 = timeout command exceeded time limit
        # 137 = killed by SIGKILL (128 + 9)
        if [[ "$status" -eq 124 || "$status" -eq 137 ]]; then
            skip "${label} timed out — network-dependent test"
        fi
        # 28 = curl CURLE_OPERATION_TIMEDOUT (--max-time exceeded)
        # output "000" = no HTTP response received (curl -w "%{http_code}" with -s flag)
        if [[ "$status" -eq 28 || "$output" == "000" ]]; then
            skip "${label} timed out — network-dependent test"
        fi
        case "$output" in
            *"Could not resolve"*|*"Connection timed out"*|\
            *"Network is unreachable"*|*"Connection refused"*)
                skip "${label} unreachable — network-dependent test"
                ;;
        esac
    fi
}
