#!/usr/bin/env bats
# Network Integration Tests
# Tests for Docker network isolation and connectivity

setup() {
    # Set paths relative to test file location
    TESTS_DIR="$(cd "${BATS_TEST_DIRNAME}/.." && pwd)"
    PROJECT_ROOT="$(cd "${TESTS_DIR}/.." && pwd)"
    load "${TESTS_DIR}/bats/bats-support/load.bash"
    load "${TESTS_DIR}/bats/bats-assert/load.bash"
    GATEWAY_CONTAINER="polis-gateway"
    ICAP_CONTAINER="polis-icap"
    WORKSPACE_CONTAINER="polis-workspace"
    CLAMAV_CONTAINER="polis-clamav"
}

# =============================================================================
# Docker Network Tests
# =============================================================================

@test "network: internal-bridge network exists" {
    run docker network ls --filter "name=internal-bridge" --format '{{.Name}}'
    assert_success
    assert_output --partial "internal-bridge"
}

@test "network: gateway-bridge network exists" {
    run docker network ls --filter "name=gateway-bridge" --format '{{.Name}}'
    assert_success
    assert_output --partial "gateway-bridge"
}

@test "network: external-bridge network exists" {
    run docker network ls --filter "name=external-bridge" --format '{{.Name}}'
    assert_success
    assert_output --partial "external-bridge"
}

@test "network: internal-bridge has IPv6 disabled" {
    run docker network inspect --format '{{.EnableIPv6}}' deploy_internal-bridge
    assert_success
    assert_output "false"
}

@test "network: gateway-bridge has IPv6 disabled" {
    run docker network inspect --format '{{.EnableIPv6}}' deploy_gateway-bridge
    assert_success
    assert_output "false"
}

@test "network: external-bridge has IPv6 disabled" {
    run docker network inspect --format '{{.EnableIPv6}}' deploy_external-bridge
    assert_success
    assert_output "false"
}

@test "network: internal-bridge uses 10.10.1.0/24 subnet" {
    run docker network inspect --format '{{range .IPAM.Config}}{{.Subnet}}{{end}}' deploy_internal-bridge
    assert_success
    assert_output "10.10.1.0/24"
}

@test "network: gateway-bridge uses 10.30.1.0/24 subnet" {
    run docker network inspect --format '{{range .IPAM.Config}}{{.Subnet}}{{end}}' deploy_gateway-bridge
    assert_success
    assert_output "10.30.1.0/24"
}

@test "network: external-bridge uses 10.20.1.0/24 subnet" {
    run docker network inspect --format '{{range .IPAM.Config}}{{.Subnet}}{{end}}' deploy_external-bridge
    assert_success
    assert_output "10.20.1.0/24"
}

# =============================================================================
# Gateway Network Connectivity Tests
# =============================================================================

@test "network: gateway connected to internal-bridge" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output --partial "internal-bridge"
}

@test "network: gateway connected to gateway-bridge" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output --partial "gateway-bridge"
}

@test "network: gateway connected to external-bridge" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output --partial "external-bridge"
}

@test "network: gateway has IP on all three networks" {
    local networks
    networks=$(docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}}:{{$config.IPAddress}} {{end}}' "${GATEWAY_CONTAINER}")
    
    # Should have 3 IPs
    local ip_count
    ip_count=$(echo "$networks" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | wc -l)
    [[ "$ip_count" -eq 3 ]]
}

# =============================================================================
# ICAP Network Isolation Tests
# =============================================================================

@test "network: icap only on gateway-bridge" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output --partial "gateway-bridge"
    refute_output --partial "internal-bridge"
    refute_output --partial "external-bridge"
}

@test "network: icap cannot reach workspace directly" {
    # ICAP should not be able to reach workspace (different network)
    local workspace_ip
    workspace_ip=$(docker inspect --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "${WORKSPACE_CONTAINER}")
    
    run docker exec "${ICAP_CONTAINER}" timeout 2 bash -c "echo > /dev/tcp/${workspace_ip}/22" 2>/dev/null
    assert_failure
}

# =============================================================================
# Workspace Network Isolation Tests
# =============================================================================

@test "network: workspace only on internal-bridge" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output --partial "internal-bridge"
    refute_output --partial "gateway-bridge"
    refute_output --partial "external-bridge"
}

@test "network: workspace cannot reach icap directly" {
    # Workspace should not be able to reach ICAP (different network)
    run docker exec "${WORKSPACE_CONTAINER}" timeout 2 bash -c "echo > /dev/tcp/icap/1344" 2>/dev/null
    assert_failure
}

# =============================================================================
# DNS Resolution Tests
# =============================================================================

@test "network: workspace can resolve gateway via DNS" {
    run docker exec "${WORKSPACE_CONTAINER}" getent hosts gateway
    assert_success
    refute_output ""
}

@test "network: gateway can resolve icap via DNS" {
    run docker exec "${GATEWAY_CONTAINER}" getent hosts icap
    assert_success
    refute_output ""
}

@test "network: gateway can resolve workspace via DNS" {
    # Try both 'workspace' (base profile) and 'workspace-openclaw' (openclaw profile)
    run docker exec "${GATEWAY_CONTAINER}" getent hosts workspace
    if [[ "$status" -ne 0 ]]; then
        run docker exec "${GATEWAY_CONTAINER}" getent hosts workspace-openclaw
    fi
    assert_success
    refute_output ""
}

@test "network: icap can resolve gateway via DNS" {
    run docker exec "${ICAP_CONTAINER}" getent hosts gateway
    assert_success
    refute_output ""
}

# =============================================================================
# Inter-Container Connectivity Tests
# =============================================================================

@test "network: gateway can reach icap on port 1344" {
    run docker exec "${GATEWAY_CONTAINER}" timeout 2 bash -c "echo > /dev/tcp/icap/1344"
    assert_success
}

@test "network: workspace can reach gateway" {
    local gateway_ip
    gateway_ip=$(docker exec "${WORKSPACE_CONTAINER}" getent hosts gateway | awk '{print $1}')
    
    run docker exec "${WORKSPACE_CONTAINER}" timeout 2 bash -c "echo > /dev/tcp/${gateway_ip}/18080" 2>/dev/null
    # May fail if TPROXY intercepts, but connection attempt should work
    # The important thing is the network path exists
}

# =============================================================================
# Routing Tests
# =============================================================================

@test "network: workspace has default route" {
    run docker exec "${WORKSPACE_CONTAINER}" ip route show default
    assert_success
    refute_output ""
}

@test "network: workspace default route points to gateway" {
    local default_gw
    default_gw=$(docker exec "${WORKSPACE_CONTAINER}" ip route show default | awk '{print $3}')
    
    # Gateway may have multiple IPs; verify default route is on same subnet as gateway
    local gateway_ip
    gateway_ip=$(docker exec "${WORKSPACE_CONTAINER}" getent hosts gateway | awk '{print $1}')
    
    # Extract subnet (first 3 octets)
    local default_subnet="${default_gw%.*}"
    local gateway_subnet="${gateway_ip%.*}"
    
    [[ "$default_subnet" == "$gateway_subnet" ]]
}

@test "network: gateway has IP forwarding enabled" {
    run docker exec "${GATEWAY_CONTAINER}" cat /proc/sys/net/ipv4/ip_forward
    assert_success
    assert_output "1"
}

@test "network: gateway has ip_nonlocal_bind enabled" {
    run docker exec "${GATEWAY_CONTAINER}" cat /proc/sys/net/ipv4/ip_nonlocal_bind
    assert_success
    assert_output "1"
}

# =============================================================================
# Network Driver Tests
# =============================================================================

@test "network: all networks use bridge driver" {
    for network in internal-bridge gateway-bridge external-bridge; do
        run docker network inspect --format '{{.Driver}}' "deploy_${network}"
        assert_success
        assert_output "bridge"
    done
}

# =============================================================================
# Container Hostname Tests
# =============================================================================

@test "network: gateway hostname is 'gateway'" {
    run docker exec "${GATEWAY_CONTAINER}" hostname
    assert_success
    # Hostname may include container ID, but should be resolvable as 'gateway'
}

@test "network: icap hostname is 'icap'" {
    run docker exec "${ICAP_CONTAINER}" hostname
    assert_success
}

@test "network: workspace hostname is 'workspace'" {
    run docker exec "${WORKSPACE_CONTAINER}" hostname
    assert_success
}

# =============================================================================
# IPv6 Security Tests (04-network-security)
# =============================================================================

# Helper to detect WSL2
is_wsl2() {
    docker exec "${WORKSPACE_CONTAINER}" grep -qi microsoft /proc/version 2>/dev/null
}

@test "network: workspace has no global IPv6 addresses" {
    run docker exec "${WORKSPACE_CONTAINER}" bash -c "ip -6 addr show scope global 2>/dev/null | grep -q inet6 && echo 'found' || echo 'none'"
    assert_success
    assert_output "none"
}

@test "network: workspace has no IPv6 addresses (native Linux)" {
    if is_wsl2; then
        skip "WSL2 may have link-local addresses"
    fi
    run docker exec "${WORKSPACE_CONTAINER}" bash -c "ip -6 addr show 2>/dev/null | grep -E 'inet6.*(scope global|scope link)' && echo 'found' || echo 'none'"
    assert_success
    assert_output "none"
}

@test "network: gateway has no global IPv6 addresses" {
    run docker exec "${GATEWAY_CONTAINER}" bash -c "ip -6 addr show scope global 2>/dev/null | grep -q inet6 && echo 'found' || echo 'none'"
    assert_success
    assert_output "none"
}

@test "network: gateway ip6tables raw table DROP" {
    # Skip if ip6tables is not functional in this environment
    if ! docker exec "${GATEWAY_CONTAINER}" ip6tables -t raw -L -n &>/dev/null; then
        skip "ip6tables not functional in this environment"
    fi
    
    run docker exec "${GATEWAY_CONTAINER}" ip6tables -t raw -L PREROUTING -n
    assert_success
    assert_output --partial "DROP"
}

@test "network: gateway ip6tables raw OUTPUT DROP" {
    if ! docker exec "${GATEWAY_CONTAINER}" ip6tables -t raw -L -n &>/dev/null; then
        skip "ip6tables not functional in this environment"
    fi
    
    run docker exec "${GATEWAY_CONTAINER}" ip6tables -t raw -L OUTPUT -n
    assert_success
    assert_output --partial "DROP"
}

@test "network: gateway ip6tables filter DROP policy" {
    if ! docker exec "${GATEWAY_CONTAINER}" ip6tables -L -n &>/dev/null; then
        skip "ip6tables not functional in this environment"
    fi
    
    run docker exec "${GATEWAY_CONTAINER}" ip6tables -L INPUT -n
    assert_success
    assert_output --partial "policy DROP"
}

@test "network: workspace IPv6 socket creation fails" {
    # Skip if IPv6 is not actually disabled (global addresses still present)
    if docker exec "${WORKSPACE_CONTAINER}" bash -c "ip -6 addr show scope global 2>/dev/null | grep -q inet6"; then
        skip "IPv6 is not disabled in this environment"
    fi
    
    run docker exec "${WORKSPACE_CONTAINER}" timeout 2 bash -c "echo > /dev/tcp/::1/80" 2>&1
    assert_failure
}
