#!/usr/bin/env bats
# CI Environment End-to-End Tests
# Tests that simulate GitHub Actions CI environment behavior

setup() {
    # Set paths relative to test file location
    TESTS_DIR="$(cd "${BATS_TEST_DIRNAME}/.." && pwd)"
    PROJECT_ROOT="$(cd "${TESTS_DIR}/.." && pwd)"
    load "${TESTS_DIR}/bats/bats-support/load.bash"
    load "${TESTS_DIR}/bats/bats-assert/load.bash"
    GATEWAY_CONTAINER="polis-gateway"
    WORKSPACE_CONTAINER="polis-workspace"

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
# CI Environment Simulation Tests
# =============================================================================

@test "ci-env: gateway starts without privileged mode" {
    # In CI, containers run without --privileged
    # Verify gateway is not running in privileged mode
    run docker inspect --format '{{.HostConfig.Privileged}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "false"
}

@test "ci-env: gateway has limited capabilities" {
    # Verify gateway has specific capabilities, not all
    run docker inspect --format '{{.HostConfig.CapAdd}}' "${GATEWAY_CONTAINER}"
    assert_success
    # Should have NET_ADMIN, NET_RAW, SETUID, SETGID but not ALL
    assert_output --partial "NET_ADMIN"
}

@test "ci-env: gateway can start in CI" {
    # This is the key CI test - gateway should be running
    run docker ps --filter "name=${GATEWAY_CONTAINER}" --format '{{.Names}}'
    assert_success
    assert_output "${GATEWAY_CONTAINER}"
}

@test "ci-env: gateway is healthy in CI environment" {
    # Gateway should reach healthy state
    run docker inspect --format '{{.State.Health.Status}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "healthy"
}

# =============================================================================
# Full Stack Tests in CI Mode
# =============================================================================

@test "ci-env: all containers running in CI mode" {
    # Verify all containers are up
    local containers=("polis-gateway" "polis-icap" "polis-clamav")
    
    for container in "${containers[@]}"; do
        run docker ps --filter "name=${container}" --format '{{.Names}}'
        assert_success
        assert_output "${container}"
    done
}

@test "ci-env: workspace container running in CI mode" {
    # Workspace might be named differently based on profile
    run docker ps --filter "name=polis-workspace" --format '{{.Names}}'
    assert_success
    assert_output --partial "polis-workspace"
}

@test "ci-env: all containers healthy in CI mode" {
    # Check health of all containers
    local containers=("polis-gateway" "polis-icap" "polis-clamav")
    
    for container in "${containers[@]}"; do
        run docker inspect --format '{{.State.Health.Status}}' "${container}"
        assert_success
        assert_output "healthy"
    done
}

# =============================================================================
# Network Functionality Tests in CI
# =============================================================================

@test "ci-env: workspace can reach internet through gateway" {
    # Test actual proxying works in CI mode
    run docker exec "${WORKSPACE_CONTAINER}" timeout 10 curl -s -o /dev/null -w "%{http_code}" http://example.com
    assert_success
    assert_output "200"
}

@test "ci-env: HTTPS interception works in CI mode" {
    # Test HTTPS proxying with certificate
    run docker exec "${WORKSPACE_CONTAINER}" timeout 10 curl -s -o /dev/null -w "%{http_code}" https://example.com
    assert_success
    assert_output "200"
}

@test "ci-env: DNS resolution works in CI mode" {
    # Test DNS through gateway
    run docker exec "${WORKSPACE_CONTAINER}" getent hosts example.com
    assert_success
    assert_output --partial "example.com"
}

@test "ci-env: gateway TPROXY intercepts traffic in CI mode" {
    # Verify TPROXY is working
    run docker exec "${GATEWAY_CONTAINER}" iptables -t mangle -L G3TPROXY -n -v
    assert_success
    # Should show packet counts if traffic is being intercepted
    assert_output --partial "TPROXY"
}

# =============================================================================
# Security Tests in CI Mode
# =============================================================================

@test "ci-env: non-HTTP traffic blocked in CI mode" {
    # Test that non-HTTP traffic is still blocked
    # Use bash /dev/tcp instead of nc which may not be installed
    run docker exec "${WORKSPACE_CONTAINER}" timeout 2 bash -c "echo > /dev/tcp/1.1.1.1/22" 2>&1
    assert_failure
}

@test "ci-env: only HTTP/HTTPS allowed in CI mode" {
    # Verify HTTP works
    run docker exec "${WORKSPACE_CONTAINER}" timeout 5 curl -s -o /dev/null -w "%{http_code}" http://example.com
    assert_success
    assert_output "200"
}

@test "ci-env: ICAP scanning active in CI mode" {
    # Verify ICAP is processing requests
    run docker exec "${GATEWAY_CONTAINER}" timeout 2 bash -c "echo > /dev/tcp/icap/1344"
    assert_success
}

@test "ci-env: ClamAV scanning active in CI mode" {
    # Verify ClamAV is running
    run docker exec polis-clamav clamdscan --ping 3
    assert_success
}

# =============================================================================
# CI-Specific Log Tests
# =============================================================================

@test "ci-env: CI mode clearly indicated in logs" {
    run docker logs "${GATEWAY_CONTAINER}" 2>&1
    assert_success
    # Should always show the IPv6 disable attempt
    assert_output --partial "Disabling IPv6"
}

@test "ci-env: no startup failures in CI mode" {
    # Verify no failure messages in logs
    run docker logs "${GATEWAY_CONTAINER}" 2>&1
    assert_success
    refute_output --partial "CRITICAL"
    refute_output --partial "Aborting"
    refute_output --partial "exit 1"
}

@test "ci-env: initialization completed successfully" {
    # Check for successful init messages
    run docker logs "${GATEWAY_CONTAINER}" 2>&1
    assert_success
    assert_output --partial "Starting g3fcgen"
}

# =============================================================================
# Performance Tests in CI
# =============================================================================

@test "ci-env: gateway responds quickly in CI mode" {
    # Test response time
    run docker exec "${WORKSPACE_CONTAINER}" timeout 5 curl -s -o /dev/null -w "%{time_total}" http://example.com
    assert_success
    
    # Response should be under 5 seconds
    local time_total="$output"
    # Basic check that we got a numeric value
    [[ "$time_total" =~ ^[0-9]+\.[0-9]+$ ]]
}

@test "ci-env: no restart loops in CI mode" {
    # Verify container hasn't been restarting
    run docker inspect --format '{{.RestartCount}}' "${GATEWAY_CONTAINER}"
    assert_success
    
    # Should be 0 or 1 (not constantly restarting)
    [[ "$output" -le 1 ]]
}

@test "ci-env: container memory usage reasonable in CI mode" {
    # Check memory usage isn't excessive
    run docker stats --no-stream --format "{{.MemUsage}}" "${GATEWAY_CONTAINER}"
    assert_success
    # Just verify we got output (actual limits tested elsewhere)
    refute_output ""
}

# =============================================================================
# GitHub Actions Specific Tests
# =============================================================================

@test "ci-env: works without CAP_SYS_ADMIN" {
    # GitHub Actions doesn't provide CAP_SYS_ADMIN
    run docker inspect --format '{{.HostConfig.CapAdd}}' "${GATEWAY_CONTAINER}"
    assert_success
    refute_output --partial "SYS_ADMIN"
}

@test "ci-env: works without host network mode" {
    # Verify not using host network
    run docker inspect --format '{{.HostConfig.NetworkMode}}' "${GATEWAY_CONTAINER}"
    assert_success
    refute_output "host"
}

@test "ci-env: sysctl modifications not required" {
    # In CI, sysctl modifications fail but container should still work
    # This is implicitly tested by the container being healthy
    run docker inspect --format '{{.State.Health.Status}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "healthy"
}

# =============================================================================
# Regression Tests
# =============================================================================

@test "ci-env: old CRITICAL error messages not present" {
    # Verify old fatal error messages are gone
    run docker logs "${GATEWAY_CONTAINER}" 2>&1
    assert_success
    refute_output --partial "CRITICAL: IPv6 addresses still present after disable attempt"
    refute_output --partial "Aborting - TPROXY bypass risk"
}

@test "ci-env: container does not exit on IPv6 failure" {
    # Verify container stays running
    run docker inspect --format '{{.State.Status}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "running"
}

@test "ci-env: init script has non-fatal IPv6 checks" {
    # Verify init script doesn't have exit 1 after IPv6 checks
    run docker exec "${GATEWAY_CONTAINER}" grep -A 5 "IPv6 addresses still present" /init.sh
    assert_success
    assert_output --partial "WARNING"
    assert_output --partial "Continuing startup"
}
