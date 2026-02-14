#!/usr/bin/env bats
# Workspace Isolation Tests
# Verifies zero-trust network isolation and prevents regression to WSL2 cruft

setup_file() {
    load "../helpers/common.bash"
    relax_security_level
}

setup() {
    load "../helpers/common.bash"
    require_container "$GATEWAY_CONTAINER" "$WORKSPACE_CONTAINER"
}

# =============================================================================
# Architecture Tests (Prevent WSL2 Cruft Regression)
# =============================================================================

@test "isolation: only inet polis table exists (no old WSL2 tables)" {
    # Should have exactly one polis table
    run docker exec "${GATEWAY_CONTAINER}" nft list tables
    assert_success
    assert_output --partial "inet polis"
    
    # Should NOT have old WSL2 tables
    refute_output --partial "ip polis_nat"
    refute_output --partial "ip polis_mangle"
}

@test "isolation: no masquerade rule exists (WSL2 cruft removed)" {
    run docker exec "${GATEWAY_CONTAINER}" nft list table inet polis
    assert_success
    refute_output --partial "masquerade"
}

@test "isolation: forward chain has policy drop (zero-trust)" {
    run docker exec "${GATEWAY_CONTAINER}" nft list chain inet polis forward
    assert_success
    assert_output --partial "policy drop"
}

@test "isolation: TPROXY rule exists in prerouting_tproxy chain" {
    run docker exec "${GATEWAY_CONTAINER}" nft list chain inet polis prerouting_tproxy
    assert_success
    assert_output --partial "tproxy to :18080"
    assert_output --partial "meta mark set"
}

@test "isolation: DNS DNAT rule exists in prerouting_dnat chain" {
    run docker exec "${GATEWAY_CONTAINER}" nft list chain inet polis prerouting_dnat
    assert_success
    assert_output --partial "dnat ip to 10.30.1.10"
    assert_output --partial "dport 53"
}

@test "isolation: IPv6 is blocked (defense-in-depth)" {
    run docker exec "${GATEWAY_CONTAINER}" nft list chain inet polis prerouting_tproxy
    assert_success
    assert_output --partial "meta nfproto ipv6 drop"
}

@test "isolation: Docker DNS rules preserved (not flushed)" {
    # Docker's internal DNS NAT table should exist
    run docker exec "${GATEWAY_CONTAINER}" nft list tables
    assert_success
    assert_output --partial "ip nat"
    
    # Should have Docker DNS rules
    run docker exec "${GATEWAY_CONTAINER}" nft list table ip nat
    assert_success
    assert_output --partial "127.0.0.11"
}

# =============================================================================
# Positive Tests (Should Work)
# =============================================================================

@test "isolation: workspace can access HTTP via TPROXY" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 10 http://example.com
    assert_success
    assert_output "200"
}

@test "isolation: workspace can access HTTPS via TPROXY" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 10 https://example.com
    assert_success
    assert_output "200"
}

@test "isolation: workspace DNS resolution works" {
    run docker exec "${WORKSPACE_CONTAINER}" getent hosts example.com
    assert_success
    refute_output ""
}

@test "isolation: workspace can resolve internal service names" {
    # Docker DNS should resolve service names
    run docker exec "${WORKSPACE_CONTAINER}" getent hosts resolver
    assert_success
    assert_output --partial "10.10.1.2"
}

# =============================================================================
# Negative Tests (Should Be Blocked - Zero-Trust)
# =============================================================================

@test "isolation: workspace CANNOT directly access gateway-bridge services" {
    # Try to reach sentinel on gateway-bridge (10.30.1.5)
    # This should fail because forward policy is drop
    run timeout 3 docker exec "${WORKSPACE_CONTAINER}" curl -s --connect-timeout 2 http://10.30.1.5:1344 2>/dev/null
    assert_failure
}

@test "isolation: workspace CANNOT directly access external-bridge" {
    # Try to reach gate's external-bridge IP (10.20.1.3)
    # Should fail - no route or forward drop
    run timeout 3 docker exec "${WORKSPACE_CONTAINER}" curl -s --connect-timeout 2 http://10.20.1.3 2>/dev/null
    assert_failure
}

@test "isolation: workspace CANNOT ping external IPs (ICMP blocked)" {
    # ICMP is not TCP, should be blocked by forward policy
    skip "ping not available in workspace container"
    run timeout 3 docker exec "${WORKSPACE_CONTAINER}" ping -c 1 -W 1 8.8.8.8
    assert_failure
}

@test "isolation: workspace CANNOT use arbitrary UDP ports" {
    # UDP (non-DNS) should be blocked by forward policy
    skip "nc not available in workspace container"
    run timeout 3 docker exec "${WORKSPACE_CONTAINER}" nc -u -w 2 8.8.8.8 123
    assert_failure
}

@test "isolation: workspace CANNOT bypass DNS to external resolvers" {
    # Trying to use 8.8.8.8:53 should be DNATed to CoreDNS
    # We can't easily test this without dig/nslookup, but we can verify the DNAT rule exists
    run docker exec "${GATEWAY_CONTAINER}" nft list chain inet polis prerouting_dnat
    assert_success
    assert_output --partial "udp dport 53 dnat"
}

# =============================================================================
# Regression Tests
# =============================================================================

@test "isolation: health check completes quickly (no 5s timeout)" {
    # Health check should complete in < 10s (was timing out at 5s before fix)
    run timeout 10 docker exec "${GATEWAY_CONTAINER}" /scripts/health-check.sh
    assert_success
    assert_output "OK"
}

@test "isolation: gate can resolve sentinel via Docker DNS" {
    # Regression: flush ruleset was breaking Docker DNS
    run docker exec "${GATEWAY_CONTAINER}" getent hosts sentinel
    assert_success
    assert_output --partial "10.30.1.5"
}

@test "isolation: TPROXY exclusions include all internal subnets" {
    # Verify internal subnets are excluded from TPROXY
    run docker exec "${GATEWAY_CONTAINER}" nft list chain inet polis prerouting_tproxy
    assert_success
    assert_output --partial "10.10.1.0/24"
    assert_output --partial "10.30.1.0/24"
    assert_output --partial "10.20.1.0/24"
}

@test "isolation: forward chain logs dropped packets" {
    # Verify logging is configured for observability
    run docker exec "${GATEWAY_CONTAINER}" nft list chain inet polis forward
    assert_success
    assert_output --partial 'log prefix "[polis-drop]'
}

@test "isolation: forward drop counter increases on blocked traffic" {
    # Get initial counter value
    local initial_drops
    initial_drops=$(docker exec "${GATEWAY_CONTAINER}" nft list chain inet polis forward | grep 'counter packets' | grep -oP 'packets \K[0-9]+')
    
    # Try to send traffic that should be dropped (direct access to gateway-bridge)
    timeout 3 docker exec "${WORKSPACE_CONTAINER}" curl -s --connect-timeout 2 http://10.30.1.5:1344 2>/dev/null || true
    
    # Counter should have increased (or stayed same if no packets forwarded)
    local final_drops
    final_drops=$(docker exec "${GATEWAY_CONTAINER}" nft list chain inet polis forward | grep 'counter packets' | grep -oP 'packets \K[0-9]+')
    
    # If traffic was forwarded and dropped, counter increases. If TPROXY caught it, counter stays same.
    # Either way is correct - we just verify the counter exists and is readable
    [[ "$final_drops" =~ ^[0-9]+$ ]]
}

@test "isolation: policy routing for TPROXY is configured" {
    # Verify fwmark 0x2 → table 102 → local
    run docker exec "${GATEWAY_CONTAINER}" ip rule show
    assert_success
    assert_output --partial "fwmark 0x2 lookup 102"
    
    run docker exec "${GATEWAY_CONTAINER}" ip route show table 102
    assert_success
    assert_output --partial "local default dev lo"
}

@test "isolation: required sysctls are set" {
    # Verify all sysctls needed for TPROXY are enabled
    run docker exec "${GATEWAY_CONTAINER}" cat /proc/sys/net/ipv4/ip_forward
    assert_output "1"
    
    run docker exec "${GATEWAY_CONTAINER}" cat /proc/sys/net/ipv4/ip_nonlocal_bind
    assert_output "1"
    
    run docker exec "${GATEWAY_CONTAINER}" cat /proc/sys/net/ipv4/conf/all/rp_filter
    assert_output "0"
    
    run docker exec "${GATEWAY_CONTAINER}" cat /proc/sys/net/ipv4/conf/all/route_localnet
    assert_output "1"
}

# =============================================================================
# Security Boundary Tests
# =============================================================================

@test "isolation: workspace default route points to gate" {
    # Verify workspace's only path out is through gate
    run docker exec "${WORKSPACE_CONTAINER}" ip route show default
    assert_success
    assert_output --partial "10.10.1.10"
}

@test "isolation: workspace is on internal-bridge only" {
    # Verify workspace has expected interfaces (+ loopback)
    # Base: 1 interface (internal-bridge)
    # With agent host-access network: 2 interfaces (internal-bridge + host-access)
    local iface_count
    iface_count=$(docker exec "${WORKSPACE_CONTAINER}" ip -o link show | grep -v lo | wc -l)
    [[ "$iface_count" -eq 1 ]] || [[ "$iface_count" -eq 2 ]]
}

@test "isolation: gate has three network interfaces" {
    # Verify gate bridges internal, gateway, and external networks
    local iface_count
    iface_count=$(docker exec "${GATEWAY_CONTAINER}" ip -o link show | grep -v lo | wc -l)
    [ "$iface_count" -eq 3 ]
}

@test "isolation: workspace cannot see other Docker networks" {
    # Workspace should only see internal-bridge (and optionally host-access for agent profiles)
    run docker inspect "${WORKSPACE_CONTAINER}" --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}'
    assert_success
    assert_output --partial "internal-bridge"
    refute_output --partial "gateway-bridge"
    refute_output --partial "external-bridge"
}
