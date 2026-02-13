#!/usr/bin/env bats
# ICAP Container Unit Tests
# Tests for polis-icap container

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$ICAP_CONTAINER"
}

# =============================================================================
# Container State Tests
# =============================================================================

@test "icap: container exists" {
    run docker ps -a --filter "name=${ICAP_CONTAINER}" --format '{{.Names}}'
    assert_success
    assert_output "${ICAP_CONTAINER}"
}

@test "icap: container is running" {
    run docker ps --filter "name=${ICAP_CONTAINER}" --format '{{.Status}}'
    assert_success
    assert_output --partial "Up"
}

@test "icap: container is healthy" {
    run docker inspect --format '{{.State.Health.Status}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output "healthy"
}

# =============================================================================
# Binary Tests
# =============================================================================

@test "icap: c-icap binary exists" {
    run docker exec "${ICAP_CONTAINER}" which c-icap
    assert_success
    assert_output "/usr/bin/c-icap"
}

@test "icap: c-icap binary is executable" {
    run docker exec "${ICAP_CONTAINER}" test -x /usr/bin/c-icap
    assert_success
}

# =============================================================================
# Process Tests
# =============================================================================

@test "icap: c-icap process is running" {
    run docker exec "${ICAP_CONTAINER}" pgrep -x c-icap
    assert_success
}

@test "icap: c-icap has multiple worker processes" {
    run docker exec "${ICAP_CONTAINER}" pgrep -c c-icap
    assert_success
    # Should have at least 2 processes (main + workers)
    [[ "$output" -ge 2 ]]
}

# =============================================================================
# Configuration Tests
# =============================================================================

@test "icap: config file exists" {
    run docker exec "${ICAP_CONTAINER}" test -f /etc/c-icap/c-icap.conf
    assert_success
}

@test "icap: config file is readable" {
    run docker exec "${ICAP_CONTAINER}" cat /etc/c-icap/c-icap.conf
    assert_success
    assert_output --partial "Port"
}

@test "icap: config specifies port 1344" {
    run docker exec "${ICAP_CONTAINER}" grep -E "^Port" /etc/c-icap/c-icap.conf
    assert_success
    assert_output --partial "1344"
}

@test "icap: echo service is configured" {
    run docker exec "${ICAP_CONTAINER}" grep "srv_echo" /etc/c-icap/c-icap.conf
    assert_success
}

# =============================================================================
# Port Tests
# =============================================================================

@test "icap: listening on TCP port 1344" {
    # Use /proc/net/tcp instead of ss (not available in minimal container)
    run docker exec "${ICAP_CONTAINER}" sh -c "cat /proc/net/tcp | grep ':0540'"
    assert_success
}

@test "icap: port 1344 bound to all interfaces" {
    # 0540 = 1344 in hex, 00000000 = 0.0.0.0
    run docker exec "${ICAP_CONTAINER}" sh -c "cat /proc/net/tcp | grep '00000000:0540'"
    assert_success
}

# =============================================================================
# User/Permission Tests
# =============================================================================

@test "icap: c-icap user exists" {
    run docker exec "${ICAP_CONTAINER}" id c-icap
    assert_success
}

@test "icap: c-icap group exists" {
    run docker exec "${ICAP_CONTAINER}" getent group c-icap
    assert_success
}

@test "icap: c-icap process runs as c-icap user" {
    run docker exec "${ICAP_CONTAINER}" ps -o user= -p $(docker exec "${ICAP_CONTAINER}" pgrep -x c-icap | head -1)
    assert_success
    assert_output "c-icap"
}

# =============================================================================
# Directory Tests
# =============================================================================

@test "icap: /var/run/c-icap directory exists" {
    run docker exec "${ICAP_CONTAINER}" test -d /var/run/c-icap
    assert_success
}

@test "icap: /var/run/c-icap owned by c-icap" {
    run docker exec "${ICAP_CONTAINER}" stat -c '%U' /var/run/c-icap
    assert_success
    assert_output "c-icap"
}

@test "icap: /etc/c-icap directory exists" {
    run docker exec "${ICAP_CONTAINER}" test -d /etc/c-icap
    assert_success
}

# =============================================================================
# Runtime Files Tests
# =============================================================================

@test "icap: PID file exists" {
    run docker exec "${ICAP_CONTAINER}" test -f /var/run/c-icap/c-icap.pid
    assert_success
}

@test "icap: PID file contains valid PID" {
    local pid
    pid=$(docker exec "${ICAP_CONTAINER}" cat /var/run/c-icap/c-icap.pid)
    run docker exec "${ICAP_CONTAINER}" ps -p "$pid"
    assert_success
}

# =============================================================================
# Entrypoint Tests
# =============================================================================

@test "icap: entrypoint script exists" {
    run docker exec "${ICAP_CONTAINER}" test -f /entrypoint.sh
    assert_success
}

@test "icap: entrypoint script is executable" {
    run docker exec "${ICAP_CONTAINER}" test -x /entrypoint.sh
    assert_success
}

# =============================================================================
# Module Tests
# =============================================================================

@test "icap: echo service module exists" {
    run docker exec "${ICAP_CONTAINER}" find /usr -name "srv_echo.so" -type f
    assert_success
    refute_output ""
}

# =============================================================================
# Log Tests
# =============================================================================

@test "icap: server log directory is writable" {
    run docker exec -u c-icap "${ICAP_CONTAINER}" test -w /var/log/c-icap
    assert_success
}

# =============================================================================
# Network Isolation Tests
# =============================================================================

@test "icap: no ports exposed to host" {
    run docker port "${ICAP_CONTAINER}"
    # Should return empty (no published ports)
    assert_output ""
}

@test "icap: only on gateway-bridge network" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${ICAP_CONTAINER}"
    assert_success
    # Should only contain gateway-bridge (and possibly default)
    assert_output --partial "gateway-bridge"
    refute_output --partial "internal-bridge"
    refute_output --partial "external-bridge"
}
