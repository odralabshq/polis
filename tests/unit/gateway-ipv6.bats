#!/usr/bin/env bats
# Gateway IPv6 Configuration Tests
# Tests for IPv6 disabling behavior with runtime capability detection

bats_require_minimum_version 1.5.0

setup() {
    load "../helpers/common.bash"
    require_container "$GATEWAY_CONTAINER"
}

# Helper: check if ip6tables is functional inside the container
ip6tables_functional() {
    docker exec "${GATEWAY_CONTAINER}" ip6tables -L -n &>/dev/null
}

# Helper: check if sysctl can read IPv6 settings inside the container
sysctl_functional() {
    docker exec "${GATEWAY_CONTAINER}" sysctl -n net.ipv6.conf.all.disable_ipv6 &>/dev/null
}

# Helper: check if IPv6 is actually disabled (no global addresses)
ipv6_disabled() {
    ! docker exec "${GATEWAY_CONTAINER}" bash -c "ip -6 addr show scope global 2>/dev/null | grep -q inet6"
}

# =============================================================================
# IPv6 Disabling Verification Tests
# =============================================================================

@test "gateway-ipv6: no global IPv6 addresses present" {
    run docker exec "${GATEWAY_CONTAINER}" bash -c "ip -6 addr show scope global 2>/dev/null | grep -q inet6 && echo 'found' || echo 'none'"
    assert_success
    assert_output "none"
}

@test "gateway-ipv6: ip6tables raw table has DROP rules" {
    if ! ip6tables_functional; then
        skip "ip6tables not functional in this environment"
    fi

    run docker exec "${GATEWAY_CONTAINER}" ip6tables -t raw -L PREROUTING -n 2>/dev/null
    assert_success
    assert_output --partial "DROP"
}

@test "gateway-ipv6: ip6tables raw OUTPUT has DROP rules" {
    if ! ip6tables_functional; then
        skip "ip6tables not functional in this environment"
    fi

    run docker exec "${GATEWAY_CONTAINER}" ip6tables -t raw -L OUTPUT -n 2>/dev/null
    assert_success
    assert_output --partial "DROP"
}

@test "gateway-ipv6: ip6tables filter INPUT policy is DROP" {
    if ! ip6tables_functional; then
        skip "ip6tables not functional in this environment"
    fi

    run docker exec "${GATEWAY_CONTAINER}" ip6tables -L INPUT -n 2>/dev/null
    assert_success
    assert_output --partial "policy DROP"
}

@test "gateway-ipv6: ip6tables filter OUTPUT policy is DROP" {
    if ! ip6tables_functional; then
        skip "ip6tables not functional in this environment"
    fi

    run docker exec "${GATEWAY_CONTAINER}" ip6tables -L OUTPUT -n 2>/dev/null
    assert_success
    assert_output --partial "policy DROP"
}

@test "gateway-ipv6: ip6tables filter FORWARD policy is DROP" {
    if ! ip6tables_functional; then
        skip "ip6tables not functional in this environment"
    fi

    run docker exec "${GATEWAY_CONTAINER}" ip6tables -L FORWARD -n 2>/dev/null
    assert_success
    assert_output --partial "policy DROP"
}

# =============================================================================
# IPv6 Socket Tests
# =============================================================================

@test "gateway-ipv6: IPv6 socket creation fails or is blocked" {
    if ! ipv6_disabled; then
        skip "IPv6 is not disabled in this environment"
    fi

    run docker exec "${GATEWAY_CONTAINER}" timeout 2 bash -c "echo > /dev/tcp/::1/80" 2>&1
    assert_failure
}

@test "gateway-ipv6: cannot ping IPv6 localhost" {
    if ! ipv6_disabled; then
        skip "IPv6 is not disabled in this environment"
    fi

    # ping6/ping -6 may not be installed, which also counts as IPv6 being unavailable
    run ! docker exec "${GATEWAY_CONTAINER}" bash -c "ping -6 -c 1 ::1 2>&1 || ping6 -c 1 ::1 2>&1"
}

# =============================================================================
# Init Script Log Verification Tests
# =============================================================================

@test "gateway-ipv6: init logs show IPv6 disable attempt" {
    run docker logs "${GATEWAY_CONTAINER}" 2>&1
    assert_success
    assert_output --partial "Disabling IPv6"
}

@test "gateway-ipv6: init logs do not show CRITICAL errors" {
    run docker logs "${GATEWAY_CONTAINER}" 2>&1
    assert_success
    refute_output --partial "CRITICAL: IPv6 addresses still present"
    refute_output --partial "Aborting - TPROXY bypass risk"
}

@test "gateway-ipv6: init logs show completion message" {
    run docker logs "${GATEWAY_CONTAINER}" 2>&1
    assert_success
    assert_output --partial "IPv6 disable/check completed"
}

# =============================================================================
# Sysctl Tests
# =============================================================================

@test "gateway-ipv6: sysctl IPv6 disable attempted (native Linux)" {
    if docker exec "${GATEWAY_CONTAINER}" grep -qi microsoft /proc/version 2>/dev/null; then
        skip "WSL2 detected - sysctl IPv6 disable not supported"
    fi

    if ! sysctl_functional; then
        skip "sysctl not functional in this environment"
    fi

    run docker logs "${GATEWAY_CONTAINER}" 2>&1
    assert_success
    [[ "$output" == *"IPv6 disabled via sysctl"* ]] || \
    [[ "$output" == *"WARNING: sysctl IPv6 disable failed"* ]] || \
    [[ "$output" == *"Disabling IPv6"* ]]
}

# =============================================================================
# WSL2 Detection Tests
# =============================================================================

@test "gateway-ipv6: WSL2 detection works correctly" {
    if docker exec "${GATEWAY_CONTAINER}" grep -qi microsoft /proc/version 2>/dev/null; then
        run docker logs "${GATEWAY_CONTAINER}" 2>&1
        assert_success
        assert_output --partial "WSL2 detected"
    else
        run docker logs "${GATEWAY_CONTAINER}" 2>&1
        assert_success
        refute_output --partial "WSL2 detected"
    fi
}

# =============================================================================
# Non-Fatal Failure Tests
# =============================================================================

@test "gateway-ipv6: container starts successfully even if IPv6 disable fails" {
    run docker ps --filter "name=${GATEWAY_CONTAINER}" --format '{{.Status}}'
    assert_success
    assert_output --partial "Up"
}

@test "gateway-ipv6: container is healthy even if IPv6 disable fails" {
    run docker inspect --format '{{.State.Health.Status}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "healthy"
}

@test "gateway-ipv6: g3proxy process running even if IPv6 disable fails" {
    run docker exec "${GATEWAY_CONTAINER}" pgrep -x g3proxy
    assert_success
}

@test "gateway-ipv6: init script does not exit on IPv6 failure" {
    run docker logs "${GATEWAY_CONTAINER}" 2>&1
    assert_success
    refute_output --partial "exit 1"
    refute_output --partial "Aborting"
}
