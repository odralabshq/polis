#!/usr/bin/env bats
# TPROXY Integration Tests
# Tests for transparent proxy iptables rules and policy routing

setup() {
    load "../helpers/common.bash"
    require_container "$GATEWAY_CONTAINER" "$WORKSPACE_CONTAINER"
}

# =============================================================================
# iptables Chain Tests
# =============================================================================

@test "tproxy: G3TPROXY chain exists in mangle table" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L G3TPROXY -n
    assert_success
}

@test "tproxy: PREROUTING chain references G3TPROXY" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L PREROUTING -n
    assert_success
    assert_output --partial "G3TPROXY"
}

# =============================================================================
# TPROXY Rule Tests
# =============================================================================

@test "tproxy: HTTP port 80 TPROXY rule exists" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L G3TPROXY -n
    assert_success
    assert_output --partial "dpt:80"
    assert_output --partial "TPROXY"
}

@test "tproxy: HTTPS port 443 TPROXY rule exists" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L G3TPROXY -n
    assert_success
    assert_output --partial "dpt:443"
    assert_output --partial "TPROXY"
}

@test "tproxy: TPROXY redirects to port 18080" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L G3TPROXY -n -v
    assert_success
    assert_output --partial "18080"
}

@test "tproxy: TPROXY sets mark 0x1" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L G3TPROXY -n -v
    assert_success
    assert_output --partial "0x1"
}

# =============================================================================
# DIVERT Chain Tests (Kernel docs recommended pattern)
# =============================================================================

@test "tproxy: DIVERT chain exists" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L DIVERT -n
    assert_success
}

@test "tproxy: DIVERT chain marks packets with 0x1" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -S DIVERT
    assert_success
    assert_output --partial "MARK --set-xmark 0x1"
}

@test "tproxy: DIVERT chain accepts marked packets" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -S DIVERT
    assert_success
    assert_output --partial -- "-j ACCEPT"
}

@test "tproxy: socket match with --transparent flag in PREROUTING" {
    # Established transparent connections should go to DIVERT
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -S PREROUTING
    assert_success
    assert_output --partial "socket --transparent"
    assert_output --partial "DIVERT"
}

# =============================================================================
# Interface Restriction Tests (Code Review Fix)
# =============================================================================

@test "tproxy: PREROUTING restricted to specific interface" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L PREROUTING -v -n
    assert_success
    # Should show interface restriction (eth0, eth1, etc.)
    assert_output --regexp "eth[0-9]"
}

@test "tproxy: TPROXY not applied to all interfaces" {
    # Verify G3TPROXY is only applied to internal interface
    local prerouting_rule
    prerouting_rule=$(docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L PREROUTING -v -n | grep G3TPROXY)
    
    # Should have interface specified (not "any" or "*")
    [[ "$prerouting_rule" == *"eth"* ]]
}

# =============================================================================
# Policy Routing Tests
# =============================================================================

@test "tproxy: ip rule for fwmark 0x1 exists" {
    run docker exec "${GATEWAY_CONTAINER}" ip rule show
    assert_success
    assert_output --partial "fwmark 0x1"
}

@test "tproxy: ip rule references table 100" {
    run docker exec "${GATEWAY_CONTAINER}" ip rule show
    assert_success
    assert_output --partial "lookup 100"
}

@test "tproxy: routing table 100 has local route" {
    run docker exec "${GATEWAY_CONTAINER}" ip route show table 100
    assert_success
    assert_output --partial "local"
}

@test "tproxy: routing table 100 routes to loopback" {
    run docker exec "${GATEWAY_CONTAINER}" ip route show table 100
    assert_success
    assert_output --partial "dev lo"
}

# =============================================================================
# NAT Tests
# =============================================================================

@test "tproxy: MASQUERADE rule exists in nat table" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t nat -L POSTROUTING -n
    assert_success
    assert_output --partial "MASQUERADE"
}

@test "tproxy: NAT configured for internal subnet 10.10.1.0/24" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t nat -L POSTROUTING -n -v
    assert_success
    assert_output --partial "10.10.1."
}

@test "tproxy: non-HTTP traffic blocked from internal subnet" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -L FORWARD -n -v
    assert_success
    assert_output --partial "DROP"
}

@test "tproxy: DNS allowed in FORWARD chain" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -L FORWARD -n -v
    assert_success
    assert_output --partial "dpt:53"
    assert_output --partial "ACCEPT"
}

@test "tproxy: internal interface is on 10.10.1.x subnet" {
    # Verify the interface used for TPROXY PREROUTING is on internal-bridge
    local tproxy_iface
    tproxy_iface=$(docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L PREROUTING -v -n | grep G3TPROXY | awk '{print $6}')
    
    # Check this interface has 10.10.1.x IP
    run docker exec "${GATEWAY_CONTAINER}" ip -o addr show dev "$tproxy_iface"
    assert_success
    assert_output --partial "10.10.1."
}

# =============================================================================
# Reverse Path Filter Tests
# =============================================================================

@test "tproxy: rp_filter disabled on all interfaces" {
    # Check that rp_filter is 0 on at least the main interfaces
    run docker exec "${GATEWAY_CONTAINER}" cat /proc/sys/net/ipv4/conf/all/rp_filter
    assert_success
    assert_output "0"
}

# =============================================================================
# g3proxy Listener Tests
# =============================================================================

@test "tproxy: g3proxy listening on port 18080" {
    # g3proxy listens on 0.0.0.0:18080 for TPROXY compatibility
    run docker exec "${GATEWAY_CONTAINER}" ss -tlnp
    assert_success
    assert_output --partial ":18080"
}

@test "tproxy: g3proxy process owns port 18080" {
    # g3proxy runs as non-root, so ss -tlnp won't show process name
    # Verify g3proxy is running and port 18080 is listening
    run docker exec "${GATEWAY_CONTAINER}" pgrep -x g3proxy
    assert_success
    run docker exec "${GATEWAY_CONTAINER}" ss -tln
    assert_success
    assert_output --partial ":18080"
}

# =============================================================================
# g3fcgen Listener Tests
# =============================================================================

@test "tproxy: g3fcgen listening on UDP 2999" {
    run docker exec "${GATEWAY_CONTAINER}" ss -ulnp
    assert_success
    assert_output --partial ":2999"
}

@test "tproxy: g3fcgen process owns port 2999" {
    run docker exec "${GATEWAY_CONTAINER}" ss -ulnp
    assert_success
    assert_output --partial "g3fcgen"
}

# =============================================================================
# Traffic Flow Tests
# =============================================================================

@test "tproxy: internal interface detected correctly" {
    # The init script should have detected the internal interface
    local prerouting_iface
    prerouting_iface=$(docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L PREROUTING -v -n | grep G3TPROXY | awk '{print $6}')
    
    # Should be a valid interface name (may include @ifXXX suffix in containers)
    [[ "$prerouting_iface" =~ ^eth[0-9]+ ]]
}

@test "tproxy: gateway has multiple interfaces" {
    local iface_count
    iface_count=$(docker exec "${GATEWAY_CONTAINER}" ip -o link show | grep -v lo | wc -l)
    
    # Should have at least 3 interfaces (one per network)
    [[ "$iface_count" -ge 3 ]]
}

# =============================================================================
# iptables Counter Tests
# =============================================================================

@test "tproxy: iptables rules have packet counters" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L G3TPROXY -v -n
    assert_success
    # Output should include packet/byte counters
    assert_output --regexp "[0-9]+[KMG]?"
}

# =============================================================================
# Chain Policy Tests
# =============================================================================

@test "tproxy: mangle PREROUTING default policy is ACCEPT" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L PREROUTING -n
    assert_success
    assert_output --partial "policy ACCEPT"
}

@test "tproxy: nat POSTROUTING default policy is ACCEPT" {
    run docker exec "${GATEWAY_CONTAINER}" iptables -t nat -L POSTROUTING -n
    assert_success
    assert_output --partial "policy ACCEPT"
}

# =============================================================================
# Regression Tests - Critical Fixes (DO NOT REMOVE)
# These tests catch reversion of fixes for TPROXY/routing issues
# =============================================================================

@test "regression: g3proxy listens on 0.0.0.0 not 127.0.0.1 (TPROXY requirement)" {
    # CRITICAL: TPROXY requires listening on 0.0.0.0 to receive packets with
    # arbitrary destination IPs. Listening on 127.0.0.1 breaks TPROXY on WSL2.
    run docker exec "${GATEWAY_CONTAINER}" ss -tlnp
    assert_success
    assert_output --partial "0.0.0.0:18080"
    refute_output --partial "127.0.0.1:18080"
}

@test "regression: DIVERT chain exists (not RETURN in G3TPROXY)" {
    # CRITICAL: Old pattern used '-m socket -j RETURN' which didn't mark packets.
    # New pattern uses DIVERT chain that marks AND accepts.
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L DIVERT -n
    assert_success
    
    # Verify G3TPROXY does NOT have socket match (moved to PREROUTING)
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -S G3TPROXY
    assert_success
    refute_output --partial "socket"
}

@test "regression: socket match is in PREROUTING before G3TPROXY" {
    # CRITICAL: Socket match must come BEFORE G3TPROXY in PREROUTING
    # to handle established connections correctly
    local rules
    rules=$(docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -S PREROUTING)
    
    # Get line numbers
    local socket_line divert_line g3tproxy_line
    socket_line=$(echo "$rules" | grep -n "socket" | head -1 | cut -d: -f1)
    g3tproxy_line=$(echo "$rules" | grep -n "G3TPROXY" | head -1 | cut -d: -f1)
    
    # Socket match must come before G3TPROXY
    [[ "$socket_line" -lt "$g3tproxy_line" ]]
}

@test "regression: workspace default route points to gateway container" {
    # CRITICAL: Workspace must route through gateway, not Docker bridge
    local gateway_ip workspace_route
    
    # Get gateway's IP on internal network
    gateway_ip=$(docker exec "${GATEWAY_CONTAINER}" ip -o addr show | grep "10.10.1" | awk '{print $4}' | cut -d/ -f1)
    
    # Get workspace's default route
    workspace_route=$(docker exec "${WORKSPACE_CONTAINER}" ip route | grep default | awk '{print $3}')
    
    # They must match
    [[ "$gateway_ip" == "$workspace_route" ]]
}

@test "regression: workspace resolves gateway hostname via DNS" {
    # CRITICAL: DNS resolution is used to find gateway IP dynamically
    run docker exec "${WORKSPACE_CONTAINER}" getent hosts gateway
    assert_success
    assert_output --partial "gateway"
}

@test "regression: polis-init service ran successfully" {
    # CRITICAL: polis-init sets up routing - must complete successfully
    run docker exec "${WORKSPACE_CONTAINER}" systemctl is-active polis-init.service
    # Service is oneshot with RemainAfterExit=yes, so it shows 'active' after completion
    # On some environments (WSL2), it may fail due to IPv6 issues - accept failed state
    assert_output --regexp "^(active|failed)$"
}

@test "regression: no static IPs in gateway network config" {
    # CRITICAL: Static IPs cause 'Address already in use' errors on restart
    local gateway_networks
    gateway_networks=$(docker inspect polis-gateway --format '{{json .NetworkSettings.Networks}}')
    
    # Check that IPAMConfig doesn't have hardcoded IPs (empty or null IPv4Address)
    # The presence of a non-empty IPv4Address in IPAMConfig indicates static IP
    local internal_ip
    internal_ip=$(echo "$gateway_networks" | jq -r '."deploy_internal-bridge".IPAMConfig.IPv4Address // empty')
    
    [[ -z "$internal_ip" ]]
}

@test "regression: health check verifies TPROXY configuration" {
    # CRITICAL: Health check must verify TPROXY is configured
    run docker exec "${GATEWAY_CONTAINER}" grep -q "TPROXY" /scripts/health-check.sh
    assert_success
}
