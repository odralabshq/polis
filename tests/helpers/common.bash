# Polis Core Test Helpers
# Common functions and assertions for BATS tests

# Load BATS libraries
load "${BATS_LIB_PATH}/bats-support/load.bash"
load "${BATS_LIB_PATH}/bats-assert/load.bash"

# Container names
export GATEWAY_CONTAINER="polis-gateway"
export ICAP_CONTAINER="polis-icap"
export WORKSPACE_CONTAINER="polis-workspace"
export CLAMAV_CONTAINER="polis-clamav"

# Timeouts
export DEFAULT_TIMEOUT=10
export NETWORK_TIMEOUT=5

# =============================================================================
# Container Assertions
# =============================================================================

# Assert container is running
# Usage: assert_container_running "container-name"
assert_container_running() {
    local container="$1"
    run docker ps --filter "name=${container}" --format '{{.Status}}'
    assert_success
    assert_output --partial "Up"
}

# Assert container is healthy
# Usage: assert_container_healthy "container-name"
assert_container_healthy() {
    local container="$1"
    run docker inspect --format '{{.State.Health.Status}}' "${container}"
    assert_success
    assert_output "healthy"
}

# Assert container is NOT running privileged
# Usage: assert_container_not_privileged "container-name"
assert_container_not_privileged() {
    local container="$1"
    run docker inspect --format '{{.HostConfig.Privileged}}' "${container}"
    assert_success
    assert_output "false"
}

# Assert container has specific capability
# Usage: assert_has_capability "container-name" "NET_ADMIN"
assert_has_capability() {
    local container="$1"
    local capability="$2"
    run docker inspect --format '{{.HostConfig.CapAdd}}' "${container}"
    assert_success
    # Docker may report as NET_ADMIN or CAP_NET_ADMIN
    assert_output --regexp "(${capability}|CAP_${capability})"
}

# Assert container has seccomp profile
# Usage: assert_has_seccomp "container-name"
assert_has_seccomp() {
    local container="$1"
    run docker inspect --format '{{.HostConfig.SecurityOpt}}' "${container}"
    assert_success
    assert_output --partial "seccomp"
}

# Assert container runs as specific user
# Usage: assert_container_user "container-name" "username"
assert_container_user() {
    local container="$1"
    local expected_user="$2"
    run docker exec "${container}" whoami
    assert_success
    assert_output "${expected_user}"
}

# =============================================================================
# Network Assertions
# =============================================================================

# Assert port is listening in container
# Usage: assert_port_listening "container-name" "port" ["tcp"|"udp"]
assert_port_listening() {
    local container="$1"
    local port="$2"
    local proto="${3:-tcp}"
    
    if [[ "$proto" == "tcp" ]]; then
        run docker exec "${container}" ss -tlnp
    else
        run docker exec "${container}" ss -ulnp
    fi
    assert_success
    assert_output --partial ":${port}"
}

# Assert container can reach host:port
# Usage: assert_can_reach "from-container" "host" "port"
assert_can_reach() {
    local from="$1"
    local host="$2"
    local port="$3"
    
    run docker exec "${from}" timeout "${NETWORK_TIMEOUT}" bash -c "echo > /dev/tcp/${host}/${port}" 2>/dev/null
    assert_success
}

# Assert container cannot reach host:port
# Usage: assert_cannot_reach "from-container" "host" "port"
assert_cannot_reach() {
    local from="$1"
    local host="$2"
    local port="$3"
    
    run docker exec "${from}" timeout "${NETWORK_TIMEOUT}" bash -c "echo > /dev/tcp/${host}/${port}" 2>/dev/null
    assert_failure
}

# Assert DNS resolution works
# Usage: assert_dns_resolves "container-name" "hostname"
assert_dns_resolves() {
    local container="$1"
    local hostname="$2"
    
    run docker exec "${container}" getent hosts "${hostname}"
    assert_success
    refute_output ""
}

# Assert HTTP request succeeds
# Usage: assert_http_success "container-name" "url"
assert_http_success() {
    local container="$1"
    local url="$2"
    
    run docker exec "${container}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout "${NETWORK_TIMEOUT}" "${url}"
    assert_success
    assert_output "200"
}

# Assert HTTP request fails or times out
# Usage: assert_http_blocked "container-name" "url"
assert_http_blocked() {
    local container="$1"
    local url="$2"
    
    run docker exec "${container}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout "${NETWORK_TIMEOUT}" "${url}" 2>/dev/null
    # Either fails (exit code != 0) or returns non-200
    if [[ "$status" -eq 0 ]]; then
        refute_output "200"
    fi
}

# =============================================================================
# iptables Assertions
# =============================================================================

# Assert iptables chain exists
# Usage: assert_iptables_chain_exists "container-name" "table" "chain"
assert_iptables_chain_exists() {
    local container="$1"
    local table="$2"
    local chain="$3"
    
    run docker exec "${container}" iptables -t "${table}" -L "${chain}" -n
    assert_success
}

# Assert iptables rule contains pattern
# Usage: assert_iptables_rule "container-name" "table" "chain" "pattern"
assert_iptables_rule() {
    local container="$1"
    local table="$2"
    local chain="$3"
    local pattern="$4"
    
    run docker exec "${container}" iptables -t "${table}" -L "${chain}" -n
    assert_success
    assert_output --partial "${pattern}"
}

# Assert ip rule exists
# Usage: assert_ip_rule "container-name" "pattern"
assert_ip_rule() {
    local container="$1"
    local pattern="$2"
    
    run docker exec "${container}" ip rule show
    assert_success
    assert_output --partial "${pattern}"
}

# =============================================================================
# Process Assertions
# =============================================================================

# Assert process is running in container
# Usage: assert_process_running "container-name" "process-name"
assert_process_running() {
    local container="$1"
    local process="$2"
    
    run docker exec "${container}" pgrep -x "${process}"
    assert_success
}

# Assert process is NOT running in container
# Usage: assert_process_not_running "container-name" "process-name"
assert_process_not_running() {
    local container="$1"
    local process="$2"
    
    run docker exec "${container}" pgrep -x "${process}"
    assert_failure
}

# =============================================================================
# File Assertions
# =============================================================================

# Assert file exists in container
# Usage: assert_file_exists_in_container "container-name" "path"
assert_file_exists_in_container() {
    local container="$1"
    local path="$2"
    
    run docker exec "${container}" test -f "${path}"
    assert_success
}

# Assert directory exists in container
# Usage: assert_dir_exists_in_container "container-name" "path"
assert_dir_exists_in_container() {
    local container="$1"
    local path="$2"
    
    run docker exec "${container}" test -d "${path}"
    assert_success
}

# Assert file is executable in container
# Usage: assert_file_executable_in_container "container-name" "path"
assert_file_executable_in_container() {
    local container="$1"
    local path="$2"
    
    run docker exec "${container}" test -x "${path}"
    assert_success
}

# =============================================================================
# Docker Network Assertions
# =============================================================================

# Assert Docker network exists
# Usage: assert_network_exists "network-name"
assert_network_exists() {
    local network="$1"
    
    run docker network ls --filter "name=${network}" --format '{{.Name}}'
    assert_success
    assert_output --partial "${network}"
}

# Assert container is connected to network
# Usage: assert_container_on_network "container-name" "network-name"
assert_container_on_network() {
    local container="$1"
    local network="$2"
    
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${container}"
    assert_success
    assert_output --partial "${network}"
}

# =============================================================================
# Utility Functions
# =============================================================================

# Wait for container to be healthy
# Usage: wait_for_healthy "container-name" [timeout_seconds]
wait_for_healthy() {
    local container="$1"
    local timeout="${2:-60}"
    local elapsed=0
    
    while [[ $elapsed -lt $timeout ]]; do
        local status
        status=$(docker inspect --format '{{.State.Health.Status}}' "${container}" 2>/dev/null || echo "unknown")
        
        if [[ "$status" == "healthy" ]]; then
            return 0
        fi
        
        sleep 2
        elapsed=$((elapsed + 2))
    done
    
    return 1
}

# Wait for port to be listening
# Usage: wait_for_port "container-name" "port" [timeout_seconds]
wait_for_port() {
    local container="$1"
    local port="$2"
    local timeout="${3:-30}"
    local elapsed=0
    
    while [[ $elapsed -lt $timeout ]]; do
        if docker exec "${container}" ss -tlnp 2>/dev/null | grep -q ":${port}"; then
            return 0
        fi
        
        sleep 1
        elapsed=$((elapsed + 1))
    done
    
    return 1
}

# Get container IP on specific network
# Usage: get_container_ip "container-name" "network-name"
get_container_ip() {
    local container="$1"
    local network="$2"
    
    docker inspect --format "{{.NetworkSettings.Networks.${network}.IPAddress}}" "${container}" 2>/dev/null
}

# Execute command in container with timeout
# Usage: exec_with_timeout "container-name" "timeout" "command" [args...]
exec_with_timeout() {
    local container="$1"
    local timeout="$2"
    shift 2
    
    docker exec "${container}" timeout "${timeout}" "$@"
}

# Skip test if containers not running
# Usage: skip_if_containers_not_running
skip_if_containers_not_running() {
    if ! docker ps --format '{{.Names}}' | grep -q "${GATEWAY_CONTAINER}"; then
        skip "Gateway container not running"
    fi
    if ! docker ps --format '{{.Names}}' | grep -q "${ICAP_CONTAINER}"; then
        skip "ICAP container not running"
    fi
    if ! docker ps --format '{{.Names}}' | grep -q "${WORKSPACE_CONTAINER}"; then
        skip "Workspace container not running"
    fi
}
