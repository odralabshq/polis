# Polis Test Helpers — single source of truth for all test setup
#
# Usage in any .bats file:
#   setup() {
#       load "../helpers/common.bash"           # unit tests
#       load "../../helpers/common.bash"        # if nested deeper
#       require_container "$GATEWAY_CONTAINER"  # skip if not running
#   }

# ── Paths ────────────────────────────────────────────────────────────────────
TESTS_DIR="$(cd "$(dirname "${BATS_TEST_FILENAME}")/.." && pwd)"
export PROJECT_ROOT="$(cd "${TESTS_DIR}/.." && pwd)"
export TESTS_DIR
export COMPOSE_FILE="${PROJECT_ROOT}/docker-compose.yml"

# ── BATS libraries ──────────────────────────────────────────────────────────
load "${TESTS_DIR}/bats/bats-support/load.bash"
load "${TESTS_DIR}/bats/bats-assert/load.bash"

# ── Container names ─────────────────────────────────────────────────────────
export DNS_CONTAINER="polis-dns"
export GATEWAY_CONTAINER="polis-gateway"
export ICAP_CONTAINER="polis-icap"
export WORKSPACE_CONTAINER="polis-workspace"
export CLAMAV_CONTAINER="polis-clamav"
export VALKEY_CONTAINER="polis-v2-valkey"
export MCP_AGENT_CONTAINER="polis-mcp-agent"

# ── Network names ───────────────────────────────────────────────────────────
export COMPOSE_PROJECT_NAME="polis"
export NETWORK_INTERNAL="${COMPOSE_PROJECT_NAME}_internal-bridge"
export NETWORK_GATEWAY="${COMPOSE_PROJECT_NAME}_gateway-bridge"
export NETWORK_EXTERNAL="${COMPOSE_PROJECT_NAME}_external-bridge"

# ── Timeouts ────────────────────────────────────────────────────────────────
export DEFAULT_TIMEOUT=10
export NETWORK_TIMEOUT=5

# =============================================================================
# Container Guards
# =============================================================================

# Skip the current test if any listed container is not running and healthy.
# Usage: require_container "$GATEWAY_CONTAINER" "$ICAP_CONTAINER"
require_container() {
    for c in "$@"; do
        local state
        state=$(docker inspect --format '{{.State.Status}}' "$c" 2>/dev/null || echo "missing")
        if [[ "$state" != "running" ]]; then
            skip "Container ${c} not running (state: ${state})"
        fi
    done
}

# Skip if core containers (gateway, icap) are not running
# Usage: skip_if_containers_not_running
skip_if_containers_not_running() {
    require_container "$GATEWAY_CONTAINER" "$ICAP_CONTAINER"
}

# =============================================================================
# Container Assertions
# =============================================================================

assert_container_running() {
    local container="$1"
    run docker ps --filter "name=^${container}$" --format '{{.Status}}'
    assert_success
    assert_output --partial "Up"
}

assert_container_healthy() {
    local container="$1"
    run docker inspect --format '{{.State.Health.Status}}' "${container}"
    assert_success
    assert_output "healthy"
}

assert_container_not_privileged() {
    local container="$1"
    run docker inspect --format '{{.HostConfig.Privileged}}' "${container}"
    assert_success
    assert_output "false"
}

assert_has_capability() {
    local container="$1" capability="$2"
    run docker inspect --format '{{.HostConfig.CapAdd}}' "${container}"
    assert_success
    assert_output --regexp "(${capability}|CAP_${capability})"
}

assert_has_seccomp() {
    local container="$1"
    run docker inspect --format '{{.HostConfig.SecurityOpt}}' "${container}"
    assert_success
    assert_output --partial "seccomp"
}

# =============================================================================
# Network Assertions
# =============================================================================

assert_port_listening() {
    local container="$1" port="$2" proto="${3:-tcp}"
    if [[ "$proto" == "tcp" ]]; then
        run docker exec "${container}" ss -tlnp
    else
        run docker exec "${container}" ss -ulnp
    fi
    assert_success
    assert_output --partial ":${port}"
}

assert_can_reach() {
    local from="$1" host="$2" port="$3"
    run docker exec "${from}" timeout "${NETWORK_TIMEOUT}" bash -c "echo > /dev/tcp/${host}/${port}" 2>/dev/null
    assert_success
}

assert_cannot_reach() {
    local from="$1" host="$2" port="$3"
    run docker exec "${from}" timeout "${NETWORK_TIMEOUT}" bash -c "echo > /dev/tcp/${host}/${port}" 2>/dev/null
    assert_failure
}

assert_dns_resolves() {
    local container="$1" hostname="$2"
    run docker exec "${container}" getent hosts "${hostname}"
    assert_success
    refute_output ""
}

assert_http_success() {
    local container="$1" url="$2"
    run docker exec "${container}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout "${NETWORK_TIMEOUT}" "${url}"
    assert_success
    assert_output "200"
}

assert_http_blocked() {
    local container="$1" url="$2"
    run docker exec "${container}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout "${NETWORK_TIMEOUT}" "${url}" 2>/dev/null
    if [[ "$status" -eq 0 ]]; then
        refute_output "200"
    fi
}

# =============================================================================
# iptables Assertions
# =============================================================================

assert_iptables_chain_exists() {
    local container="$1" table="$2" chain="$3"
    run docker exec "${container}" iptables -t "${table}" -L "${chain}" -n
    assert_success
}

assert_iptables_rule() {
    local container="$1" table="$2" chain="$3" pattern="$4"
    run docker exec "${container}" iptables -t "${table}" -L "${chain}" -n
    assert_success
    assert_output --partial "${pattern}"
}

assert_ip_rule() {
    local container="$1" pattern="$2"
    run docker exec "${container}" ip rule show
    assert_success
    assert_output --partial "${pattern}"
}

# =============================================================================
# Process / File Assertions
# =============================================================================

assert_process_running() {
    local container="$1" process="$2"
    run docker exec "${container}" pgrep -x "${process}"
    assert_success
}

assert_file_exists_in_container() {
    local container="$1" path="$2"
    run docker exec "${container}" test -f "${path}"
    assert_success
}

assert_dir_exists_in_container() {
    local container="$1" path="$2"
    run docker exec "${container}" test -d "${path}"
    assert_success
}

# =============================================================================
# Docker Network Assertions
# =============================================================================

assert_network_exists() {
    local network="$1"
    run docker network ls --filter "name=${network}" --format '{{.Name}}'
    assert_success
    assert_output --partial "${network}"
}

assert_container_on_network() {
    local container="$1" network="$2"
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${container}"
    assert_success
    assert_output --partial "${network}"
}

# =============================================================================
# Utility Functions
# =============================================================================

wait_for_healthy() {
    local container="$1" timeout="${2:-60}" elapsed=0
    while [[ $elapsed -lt $timeout ]]; do
        local status
        status=$(docker inspect --format '{{.State.Health.Status}}' "${container}" 2>/dev/null || echo "unknown")
        [[ "$status" == "healthy" ]] && return 0
        sleep 2
        elapsed=$((elapsed + 2))
    done
    return 1
}

wait_for_port() {
    local container="$1" port="$2" timeout="${3:-30}" elapsed=0
    while [[ $elapsed -lt $timeout ]]; do
        docker exec "${container}" ss -tlnp 2>/dev/null | grep -q ":${port}" && return 0
        sleep 1
        elapsed=$((elapsed + 1))
    done
    return 1
}

get_container_ip() {
    local container="$1" network="$2"
    docker inspect --format "{{.NetworkSettings.Networks.${network}.IPAddress}}" "${container}" 2>/dev/null
}

exec_with_timeout() {
    local container="$1" timeout="$2"
    shift 2
    docker exec "${container}" timeout "${timeout}" "$@"
}

# Set valkey security_level to relaxed for e2e tests
# (prevents new_domain_prompt from blocking test traffic)
relax_security_level() {
    # Check if Valkey container exists and is running
    if ! docker ps --filter "name=^${VALKEY_CONTAINER}$" --format '{{.Names}}' 2>/dev/null | grep -q "^${VALKEY_CONTAINER}$"; then
        return 0  # Silently skip if Valkey not running
    fi
    
    local admin_pass
    admin_pass=$(docker exec "$VALKEY_CONTAINER" cat /run/secrets/valkey_mcp_admin_password 2>/dev/null || echo "")
    if [[ -n "$admin_pass" ]]; then
        docker exec "$VALKEY_CONTAINER" sh -c "valkey-cli --tls --cert /etc/valkey/tls/client.crt \
            --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
            --user mcp-admin --pass '$admin_pass' --no-auth-warning \
            SET polis:config:security_level relaxed" 2>/dev/null || true
        
        # Restart ICAP to reload security level (it only reads on startup + poll interval)
        docker restart "$ICAP_CONTAINER" >/dev/null 2>&1 || true
        sleep 3
    fi
    return 0  # Always succeed
}
