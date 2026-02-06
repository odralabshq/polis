#!/usr/bin/env bats
# ClamAV Integration Tests
# Tests for ClamAV malware scanning via SquidClamav

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
    
    export CLAMAV_CONTAINER="polis-clamav"
}

# =============================================================================
# ClamAV Container Tests
# =============================================================================

@test "clamav: container exists" {
    run docker ps -a --filter "name=${CLAMAV_CONTAINER}" --format '{{.Names}}'
    assert_success
    assert_output "${CLAMAV_CONTAINER}"
}

@test "clamav: container is running" {
    run docker ps --filter "name=${CLAMAV_CONTAINER}" --format '{{.Status}}'
    assert_success
    assert_output --partial "Up"
}

@test "clamav: container is healthy" {
    run docker inspect --format '{{.State.Health.Status}}' "${CLAMAV_CONTAINER}"
    assert_success
    assert_output "healthy"
}

@test "clamav: uses correct image version" {
    run docker inspect --format '{{.Config.Image}}' "${CLAMAV_CONTAINER}"
    assert_success
    assert_output --partial "clamav/clamav:1.5"
}

# =============================================================================
# ClamAV Security Hardening Tests
# =============================================================================

@test "clamav: has no-new-privileges" {
    run docker inspect --format '{{.HostConfig.SecurityOpt}}' "${CLAMAV_CONTAINER}"
    assert_success
    assert_output --partial "no-new-privileges"
}

@test "clamav: has cap_drop ALL" {
    run docker inspect --format '{{.HostConfig.CapDrop}}' "${CLAMAV_CONTAINER}"
    assert_success
    assert_output --partial "ALL"
}

@test "clamav: has required capabilities added back" {
    run docker inspect --format '{{.HostConfig.CapAdd}}' "${CLAMAV_CONTAINER}"
    assert_success
    assert_output --partial "CHOWN"
    assert_output --partial "SETGID"
    assert_output --partial "SETUID"
}

@test "clamav: filesystem is read-only" {
    run docker inspect --format '{{.HostConfig.ReadonlyRootfs}}' "${CLAMAV_CONTAINER}"
    assert_success
    assert_output "true"
}

@test "clamav: has tmpfs mounts for writable directories" {
    run docker inspect --format '{{json .HostConfig.Tmpfs}}' "${CLAMAV_CONTAINER}"
    assert_success
    assert_output --partial "/tmp"
    assert_output --partial "/var/log/clamav"
    assert_output --partial "/run/clamav"
}

# =============================================================================
# ClamAV Network Tests
# =============================================================================

@test "clamav: listening on port 3310" {
    run docker exec "${CLAMAV_CONTAINER}" netstat -tlnp
    assert_success
    assert_output --partial ":3310"
}

@test "clamav: only on gateway-bridge network" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${CLAMAV_CONTAINER}"
    assert_success
    assert_output --partial "gateway-bridge"
    refute_output --partial "internal-bridge"
    refute_output --partial "external-bridge"
}

@test "clamav: no ports exposed to host" {
    run docker port "${CLAMAV_CONTAINER}"
    assert_output ""
}

# =============================================================================
# ClamAV Daemon Tests
# =============================================================================

@test "clamav: responds to PING command" {
    run docker exec "${ICAP_CONTAINER}" sh -c "echo 'PING' | nc clamav 3310"
    assert_success
    assert_output "PONG"
}

@test "clamav: returns version info" {
    run docker exec "${ICAP_CONTAINER}" sh -c "echo 'VERSION' | nc clamav 3310"
    assert_success
    assert_output --partial "ClamAV"
}

@test "clamav: signature database is loaded" {
    run docker exec "${CLAMAV_CONTAINER}" ls /var/lib/clamav/
    assert_success
    assert_output --partial "main.cvd"
    assert_output --partial "daily.cvd"
}

# =============================================================================
# ClamAV Resource Limits Tests
# =============================================================================

@test "clamav: has memory limit configured" {
    run docker inspect --format '{{.HostConfig.Memory}}' "${CLAMAV_CONTAINER}"
    assert_success
    # 3GB = 3221225472 bytes
    [[ "$output" -eq 3221225472 ]]
}

@test "clamav: has memory reservation configured" {
    run docker inspect --format '{{.HostConfig.MemoryReservation}}' "${CLAMAV_CONTAINER}"
    assert_success
    # 1GB = 1073741824 bytes
    [[ "$output" -eq 1073741824 ]]
}

# =============================================================================
# SquidClamav Service Tests
# =============================================================================

@test "squidclamav: module is loaded in c-icap" {
    run docker exec "${ICAP_CONTAINER}" grep "squidclamav" /etc/c-icap/c-icap.conf
    assert_success
    assert_output --partial "squidclamav.so"
}

@test "squidclamav: config file exists" {
    run docker exec "${ICAP_CONTAINER}" test -f /etc/squidclamav.conf
    assert_success
}

@test "squidclamav: config points to clamav host" {
    run docker exec "${ICAP_CONTAINER}" grep "clamd_ip" /etc/squidclamav.conf
    assert_success
    assert_output --partial "clamav"
}

@test "squidclamav: config uses port 3310" {
    run docker exec "${ICAP_CONTAINER}" grep "clamd_port" /etc/squidclamav.conf
    assert_success
    assert_output --partial "3310"
}

@test "squidclamav: ICAP service responds to OPTIONS" {
    run docker exec "${ICAP_CONTAINER}" sh -c "printf 'OPTIONS icap://localhost:1344/squidclamav ICAP/1.0\r\nHost: localhost\r\n\r\n' | nc localhost 1344 | head -1"
    assert_success
    assert_output --partial "200"
}

@test "squidclamav: service reports correct methods" {
    run docker exec "${ICAP_CONTAINER}" sh -c "printf 'OPTIONS icap://localhost:1344/squidclamav ICAP/1.0\r\nHost: localhost\r\n\r\n' | nc localhost 1344"
    assert_success
    assert_output --partial "Methods: RESPMOD, REQMOD"
}

@test "squidclamav: service identifies as SquidClamav" {
    run docker exec "${ICAP_CONTAINER}" sh -c "printf 'OPTIONS icap://localhost:1344/squidclamav ICAP/1.0\r\nHost: localhost\r\n\r\n' | nc localhost 1344"
    assert_success
    assert_output --partial "SquidClamav"
}

# =============================================================================
# g3proxy ICAP Configuration Tests
# =============================================================================

@test "g3proxy: RESPMOD configured for squidclamav" {
    run docker exec "${GATEWAY_CONTAINER}" grep "squidclamav" /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "squidclamav"
}

@test "g3proxy: REQMOD configured for echo" {
    run docker exec "${GATEWAY_CONTAINER}" grep "icap_reqmod_service" /etc/g3proxy/g3proxy.yaml
    assert_success
    assert_output --partial "echo"
}

# =============================================================================
# Health Check Tests
# =============================================================================

@test "healthcheck: gateway checks squidclamav service" {
    run docker exec "${GATEWAY_CONTAINER}" cat /scripts/health-check.sh
    assert_success
    assert_output --partial "squidclamav"
}

@test "healthcheck: gateway can reach ICAP squidclamav" {
    run docker exec "${GATEWAY_CONTAINER}" sh -c "printf 'OPTIONS icap://icap:1344/squidclamav ICAP/1.0\r\nHost: icap\r\n\r\n' | timeout 3 nc icap 1344 | head -1"
    assert_success
    assert_output --partial "200"
}

@test "healthcheck: gateway can reach ICAP echo" {
    run docker exec "${GATEWAY_CONTAINER}" sh -c "printf 'OPTIONS icap://icap:1344/echo ICAP/1.0\r\nHost: icap\r\n\r\n' | timeout 3 nc icap 1344 | head -1"
    assert_success
    assert_output --partial "200"
}

# =============================================================================
# ICAP Container Updates Tests
# =============================================================================

@test "icap: has gosu installed" {
    run docker exec "${ICAP_CONTAINER}" which gosu
    assert_success
}

@test "icap: has netcat installed" {
    run docker exec "${ICAP_CONTAINER}" which nc
    assert_success
}

@test "icap: squidclamav.so module exists" {
    run docker exec "${ICAP_CONTAINER}" find /usr -name "squidclamav.so" -type f
    assert_success
    refute_output ""
}

@test "icap: depends on clamav service" {
    # Check that ICAP started after ClamAV was healthy
    local icap_start clamav_start
    icap_start=$(docker inspect --format '{{.State.StartedAt}}' "${ICAP_CONTAINER}")
    clamav_start=$(docker inspect --format '{{.State.StartedAt}}' "${CLAMAV_CONTAINER}")
    
    # ICAP should start after ClamAV
    [[ "$icap_start" > "$clamav_start" ]]
}

# =============================================================================
# Freshclam Configuration Tests
# =============================================================================

@test "clamav: freshclam.conf is mounted" {
    run docker exec "${CLAMAV_CONTAINER}" test -f /etc/clamav/freshclam.conf
    assert_success
}

@test "clamav: freshclam configured for database updates" {
    run docker exec "${CLAMAV_CONTAINER}" grep "DatabaseMirror" /etc/clamav/freshclam.conf
    assert_success
    assert_output --partial "database.clamav.net"
}

# =============================================================================
# Volume Persistence Tests
# =============================================================================

@test "clamav: database volume is mounted" {
    run docker inspect --format '{{range .Mounts}}{{if eq .Destination "/var/lib/clamav"}}{{.Name}}{{end}}{{end}}' "${CLAMAV_CONTAINER}"
    assert_success
    assert_output "polis-clamav-db"
}

@test "clamav: database volume exists" {
    run docker volume ls --filter "name=polis-clamav-db" --format '{{.Name}}'
    assert_success
    assert_output "polis-clamav-db"
}
