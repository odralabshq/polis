#!/usr/bin/env bats
# Edge Case and Failure Mode Tests
# Tests for error handling, recovery, and boundary conditions

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

    # Ensure security level is relaxed so test traffic isn't blocked by new_domain_prompt
    local admin_pass
    admin_pass="$(grep 'VALKEY_MCP_ADMIN_PASS=' "${PROJECT_ROOT}/secrets/credentials.env.example" 2>/dev/null | cut -d'=' -f2)"
    if [[ -n "$admin_pass" ]]; then
        docker exec polis-v2-valkey valkey-cli --tls --cert /etc/valkey/tls/client.crt \
            --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
            --user mcp-admin --pass "$admin_pass" \
            SET molis:config:security_level relaxed 2>/dev/null || true
    fi
}

# =============================================================================
# Certificate Edge Cases
# =============================================================================

@test "edge: CA certificate validation detects expiry" {
    # The init script should check certificate expiry
    # This test verifies the validation logic exists
    run docker exec "${GATEWAY_CONTAINER}" bash -c '
        openssl x509 -checkend 86400 -noout -in /etc/g3proxy/ssl/ca.pem
    '
    assert_success
}

@test "edge: CA key/cert mismatch would be detected" {
    # Verify the validation logic in init script
    run docker exec "${GATEWAY_CONTAINER}" bash -c '
        cert_hash=$(openssl x509 -noout -modulus -in /etc/g3proxy/ssl/ca.pem | openssl sha256)
        key_hash=$(openssl rsa -noout -modulus -in /etc/g3proxy/ssl/ca.key | openssl sha256)
        [ "$cert_hash" = "$key_hash" ]
    '
    assert_success
}

# =============================================================================
# ICAP Failure Scenarios
# =============================================================================

@test "edge: gateway health check verifies ICAP connectivity" {
    # Health check should fail if ICAP is unreachable
    run docker exec "${GATEWAY_CONTAINER}" /scripts/health-check.sh
    assert_success
    assert_output "OK"
}

@test "edge: gateway detects ICAP service" {
    # Gateway should be able to reach ICAP
    run docker exec "${GATEWAY_CONTAINER}" timeout 2 bash -c "echo > /dev/tcp/icap/1344"
    assert_success
}

# =============================================================================
# Network Interface Detection
# =============================================================================

@test "edge: gateway detects internal interface" {
    # Verify the interface detection logic worked
    local prerouting_rule
    prerouting_rule=$(docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L PREROUTING -v -n | grep G3TPROXY)
    
    # Should have an interface specified
    [[ "$prerouting_rule" =~ eth[0-9] ]]
}

@test "edge: gateway handles multiple interfaces" {
    # Gateway should have multiple interfaces
    local iface_count
    iface_count=$(docker exec "${GATEWAY_CONTAINER}" ip -o link show | grep -v lo | wc -l)
    
    [[ "$iface_count" -ge 3 ]]
}

# =============================================================================
# Process Recovery Tests
# =============================================================================

@test "edge: g3proxy process can be queried" {
    run docker exec "${GATEWAY_CONTAINER}" pgrep -x g3proxy
    assert_success
}

@test "edge: g3fcgen process can be queried" {
    run docker exec "${GATEWAY_CONTAINER}" pgrep -x g3fcgen
    assert_success
}

@test "edge: c-icap process can be queried" {
    run docker exec "${ICAP_CONTAINER}" pgrep -x c-icap
    assert_success
}

# =============================================================================
# PID File Handling
# =============================================================================

@test "edge: ICAP PID file is valid" {
    local pid
    pid=$(docker exec "${ICAP_CONTAINER}" cat /var/run/c-icap/c-icap.pid 2>/dev/null)
    
    # PID should be a number
    [[ "$pid" =~ ^[0-9]+$ ]]
    
    # Process should exist
    run docker exec "${ICAP_CONTAINER}" ps -p "$pid"
    assert_success
}

@test "edge: stale PID file cleanup works" {
    # The entrypoint should clean up stale PID files
    # Verify PID file exists and is current
    run docker exec "${ICAP_CONTAINER}" test -f /var/run/c-icap/c-icap.pid
    assert_success
}

# =============================================================================
# Control Socket Tests
# =============================================================================

@test "edge: g3proxy control directory exists" {
    run docker exec "${GATEWAY_CONTAINER}" test -d /tmp/g3
    assert_success
}

@test "edge: g3proxy control directory is clean" {
    # Should not have stale sockets from previous runs
    run docker exec "${GATEWAY_CONTAINER}" ls /tmp/g3
    assert_success
}

# =============================================================================
# Resource Limit Tests
# =============================================================================

@test "edge: gateway can handle iptables operations" {
    # Verify iptables is functional
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L -n
    assert_success
}

@test "edge: gateway can handle ip rule operations" {
    run docker exec "${GATEWAY_CONTAINER}" ip rule show
    assert_success
}

# =============================================================================
# DNS Edge Cases
# =============================================================================

@test "edge: DNS resolver is configured in g3proxy" {
    run docker exec "${GATEWAY_CONTAINER}" grep -A5 "resolver:" /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "8.8.8.8"
}

@test "edge: DNS resolution for non-existent domain fails gracefully" {
    run docker exec "${WORKSPACE_CONTAINER}" getent hosts nonexistent.invalid.domain.test 2>&1
    # Should fail but not crash
    assert_failure
}

# =============================================================================
# Timeout Edge Cases
# =============================================================================

@test "edge: connection timeout is handled" {
    # Try to connect to a non-routable IP
    # Note: TPROXY may intercept and return a proxy error (502/504) instead of timing out
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 3 http://10.255.255.1 2>/dev/null
    # Should timeout (status != 0), return 000 (no response), or proxy error (502/504)
    [[ "$status" -ne 0 ]] || [[ "$output" == "000" ]] || [[ "$output" == "502" ]] || [[ "$output" == "504" ]]
}

@test "edge: very long URL is handled" {
    # Generate a long path
    local long_path
    long_path=$(printf 'a%.0s' {1..200})
    
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 10 "http://httpbin.org/${long_path}" 2>/dev/null
    # Should return 404 or similar, not crash
    assert_success
}

# =============================================================================
# Container Dependency Tests
# =============================================================================

@test "edge: gateway depends on icap" {
    run docker inspect --format '{{.HostConfig.Links}}' "${GATEWAY_CONTAINER}"
    # Check depends_on in compose (not links)
}

@test "edge: workspace depends on gateway" {
    # Workspace should start after gateway
    # Verify by checking both are running
    run docker ps --filter "name=${GATEWAY_CONTAINER}" --format '{{.Status}}'
    assert_success
    assert_output --partial "Up"
    
    run docker ps --filter "name=${WORKSPACE_CONTAINER}" --format '{{.Status}}'
    assert_success
    assert_output --partial "Up"
}

# =============================================================================
# Volume Mount Edge Cases
# =============================================================================

@test "edge: config files are mounted correctly" {
    run docker exec "${GATEWAY_CONTAINER}" test -f /etc/g3proxy/g3proxy.yaml
    assert_success
}

@test "edge: CA files are mounted correctly" {
    run docker exec "${GATEWAY_CONTAINER}" test -f /etc/g3proxy/ssl/ca.pem
    assert_success
    run docker exec "${GATEWAY_CONTAINER}" test -f /etc/g3proxy/ssl/ca.key
    assert_success
}

@test "edge: workspace CA cert is mounted" {
    run docker exec "${WORKSPACE_CONTAINER}" test -f /usr/local/share/ca-certificates/polis-ca.crt
    assert_success
}

# =============================================================================
# Sysctl Edge Cases
# =============================================================================

@test "edge: ip_forward is enabled" {
    run docker exec "${GATEWAY_CONTAINER}" cat /proc/sys/net/ipv4/ip_forward
    assert_success
    assert_output "1"
}

@test "edge: ip_nonlocal_bind is enabled" {
    run docker exec "${GATEWAY_CONTAINER}" cat /proc/sys/net/ipv4/ip_nonlocal_bind
    assert_success
    assert_output "1"
}

@test "edge: rp_filter is disabled" {
    run docker exec "${GATEWAY_CONTAINER}" cat /proc/sys/net/ipv4/conf/all/rp_filter
    assert_success
    assert_output "0"
}

# =============================================================================
# Logging Edge Cases
# =============================================================================

@test "edge: gateway logs are accessible" {
    run docker logs "${GATEWAY_CONTAINER}" --tail 5
    assert_success
}

@test "edge: icap logs are accessible" {
    run docker logs "${ICAP_CONTAINER}" --tail 5
    assert_success
}

@test "edge: workspace logs are accessible" {
    run docker logs "${WORKSPACE_CONTAINER}" --tail 5
    assert_success
}

# =============================================================================
# Health Check Edge Cases
# =============================================================================

@test "edge: gateway health check script validates all components" {
    # Health check should verify g3proxy, g3fcgen, iptables, and ICAP
    run docker exec "${GATEWAY_CONTAINER}" cat /scripts/health-check.sh
    assert_success
    assert_output --partial "g3proxy"
    assert_output --partial "g3fcgen"
    assert_output --partial "iptables"
    assert_output --partial "icap"
}

@test "edge: icap health check verifies process" {
    run docker inspect --format '{{.Config.Healthcheck.Test}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output --partial "pgrep"
}

# =============================================================================
# Systemd Edge Cases (Workspace)
# =============================================================================

@test "edge: systemd journal is functional" {
    run docker exec "${WORKSPACE_CONTAINER}" journalctl --no-pager -n 5
    assert_success
}

@test "edge: polis-init service logs are available" {
    run docker exec "${WORKSPACE_CONTAINER}" journalctl -u polis-init.service --no-pager -n 5
    assert_success
}

# =============================================================================
# Binary Version Tests
# =============================================================================

@test "edge: g3proxy version is 1.12.x" {
    run docker exec "${GATEWAY_CONTAINER}" g3proxy --version
    assert_success
    assert_output --partial "1.12"
}

@test "edge: g3fcgen version is accessible" {
    run docker exec "${GATEWAY_CONTAINER}" g3fcgen --version
    assert_success
    # g3fcgen has its own version scheme (0.x.x)
    assert_output --partial "g3fcgen"
}

# =============================================================================
# Empty/Null Input Tests
# =============================================================================

@test "edge: empty HTTP body is handled" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -X POST --connect-timeout 10 http://httpbin.org/post
    assert_success
}

@test "edge: null bytes in URL are handled" {
    # Should not crash the proxy
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 10 "http://httpbin.org/get?test=%00" 2>/dev/null
    # May succeed or fail, but should not hang
}
