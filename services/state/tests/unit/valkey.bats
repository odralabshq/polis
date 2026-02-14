#!/usr/bin/env bats
# Valkey Container Unit Tests
# Tests for polis-v2-valkey container
# Requirements: 1.1–1.8, 2.1–2.4, 7.1–7.4, 8.1–8.5

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$VALKEY_CONTAINER"
}

# =============================================================================
# Container State Tests (Requirement 1.1)
# =============================================================================

@test "valkey: container exists" {
    run docker ps -a --filter "name=^${VALKEY_CONTAINER}$" --format '{{.Names}}'
    assert_success
    assert_output "${VALKEY_CONTAINER}"
}

@test "valkey: container is running" {
    run docker ps --filter "name=${VALKEY_CONTAINER}" --format '{{.Status}}'
    assert_success
    assert_output --partial "Up"
}

@test "valkey: container is healthy" {
    run docker inspect --format '{{.State.Health.Status}}' "${VALKEY_CONTAINER}"
    assert_success
    assert_output "healthy"
}

# =============================================================================
# Security Tests (Requirements 1.4, 8.1, 8.2, 8.3)
# =============================================================================

@test "valkey: has no-new-privileges security option" {
    run docker inspect --format '{{.HostConfig.SecurityOpt}}' "${VALKEY_CONTAINER}"
    assert_success
    assert_output --partial "no-new-privileges"
}

@test "valkey: drops all capabilities" {
    run docker inspect --format '{{.HostConfig.CapDrop}}' "${VALKEY_CONTAINER}"
    assert_success
    assert_output --partial "ALL"
}

@test "valkey: has read-only root filesystem" {
    run docker inspect --format '{{.HostConfig.ReadonlyRootfs}}' "${VALKEY_CONTAINER}"
    assert_success
    assert_output "true"
}

@test "valkey: has tmpfs mount at /tmp" {
    run docker inspect --format '{{json .HostConfig.Tmpfs}}' "${VALKEY_CONTAINER}"
    assert_success
    assert_output --partial "/tmp"
}

@test "valkey: is not running privileged" {
    run docker inspect --format '{{.HostConfig.Privileged}}' "${VALKEY_CONTAINER}"
    assert_success
    assert_output "false"
}

# =============================================================================
# Network Tests (Requirements 1.2, 8.5)
# =============================================================================

@test "valkey: connected to gateway-bridge network" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${VALKEY_CONTAINER}"
    assert_success
    assert_output --partial "gateway-bridge"
}

@test "valkey: not connected to internal-bridge network" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${VALKEY_CONTAINER}"
    assert_success
    refute_output --partial "internal-bridge"
}

@test "valkey: not connected to external-bridge network" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${VALKEY_CONTAINER}"
    assert_success
    refute_output --partial "external-bridge"
}

@test "valkey: no ports exposed to host" {
    run docker port "${VALKEY_CONTAINER}"
    assert_output ""
}

# =============================================================================
# Secrets Tests (Requirement 1.3)
# =============================================================================

@test "valkey: /run/secrets/valkey_password exists" {
    run docker exec "${VALKEY_CONTAINER}" test -f /run/secrets/valkey_password
    assert_success
}

@test "valkey: /run/secrets/valkey_acl exists" {
    run docker exec "${VALKEY_CONTAINER}" test -f /run/secrets/valkey_acl
    assert_success
}

# =============================================================================
# Config Tests (Requirements 1.1, 1.8, 2.1, 2.4)
# =============================================================================

@test "valkey: TLS port 6379 is listening" {
    run docker exec "${VALKEY_CONTAINER}" sh -c "cat /proc/net/tcp | grep ':18EB'"
    assert_success
}

@test "valkey: valkey.conf is mounted" {
    run docker exec "${VALKEY_CONTAINER}" test -f /etc/valkey/valkey.conf
    assert_success
}

@test "valkey: AOF persistence is enabled" {
    run docker exec "${VALKEY_CONTAINER}" grep -E "^appendonly yes" /etc/valkey/valkey.conf
    assert_success
    assert_output --partial "appendonly yes"
}

@test "valkey: non-TLS port is disabled" {
    run docker exec "${VALKEY_CONTAINER}" grep -E "^port 0" /etc/valkey/valkey.conf
    assert_success
    assert_output --partial "port 0"
}

@test "valkey: TLS certificates are mounted" {
    run docker exec "${VALKEY_CONTAINER}" test -d /etc/valkey/tls
    assert_success
}

@test "valkey: TLS auth-clients is enabled" {
    run docker exec "${VALKEY_CONTAINER}" grep -E "^tls-auth-clients yes" /etc/valkey/valkey.conf
    assert_success
    assert_output --partial "tls-auth-clients yes"
}

# =============================================================================
# Resource Tests (Requirements 1.5, 8.4)
# =============================================================================

@test "valkey: memory limit is 512M" {
    run docker inspect --format '{{.HostConfig.Memory}}' "${VALKEY_CONTAINER}"
    assert_success
    # 512MB = 536870912 bytes
    assert_output "536870912"
}

@test "valkey: CPU limit is 1.0" {
    run docker inspect --format '{{.HostConfig.NanoCpus}}' "${VALKEY_CONTAINER}"
    assert_success
    # 1.0 CPU = 1000000000 NanoCpus
    assert_output "1000000000"
}

@test "valkey: memory reservation is 256M" {
    run docker inspect --format '{{.HostConfig.MemoryReservation}}' "${VALKEY_CONTAINER}"
    assert_success
    # 256MB = 268435456 bytes
    assert_output "268435456"
}

# =============================================================================
# Volume Tests (Requirements 7.1, 7.3, 7.4)
# =============================================================================

@test "valkey: /data directory exists" {
    run docker exec "${VALKEY_CONTAINER}" test -d /data
    assert_success
}

@test "valkey: valkey-data volume is mounted at /data" {
    run docker inspect --format '{{range .Mounts}}{{if eq .Destination "/data"}}{{.Name}}{{end}}{{end}}' "${VALKEY_CONTAINER}"
    assert_success
    assert_output "polis-state-data"
}

@test "valkey: /data directory is writable" {
    run docker exec "${VALKEY_CONTAINER}" test -w /data
    assert_success
}

# =============================================================================
# Logging Tests (Requirement 1.6)
# =============================================================================

@test "valkey: uses json-file logging driver" {
    run docker inspect --format '{{.HostConfig.LogConfig.Type}}' "${VALKEY_CONTAINER}"
    assert_success
    assert_output "json-file"
}

# =============================================================================
# Restart Policy Tests (Requirement 1.7)
# =============================================================================

@test "valkey: restart policy is unless-stopped" {
    run docker inspect --format '{{.HostConfig.RestartPolicy.Name}}' "${VALKEY_CONTAINER}"
    assert_success
    assert_output "unless-stopped"
}

# =============================================================================
# Image Tests (Requirement 1.1)
# =============================================================================

@test "valkey: uses valkey/valkey:8-alpine image" {
    run docker exec "${VALKEY_CONTAINER}" grep -E "^tls-auth-clients yes" /etc/valkey/valkey.conf
    run docker inspect --format '{{.Config.Image}}' "${VALKEY_CONTAINER}"
    assert_success
    assert_output "valkey/valkey:8-alpine"
}

# =============================================================================
# ACL Tests (Requirement 7)
# =============================================================================

@test "valkey: mcp-agent ACL is tightened (no config access)" {
    # Verify mcp-agent user cannot set security level
    local mcp_pass=$(cat "${PROJECT_ROOT}/secrets/valkey_mcp_agent_password.txt")
    run docker exec "${VALKEY_CONTAINER}" valkey-cli --tls --cert /etc/valkey/tls/client.crt --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt --user mcp-agent --pass "$mcp_pass" SET polis:config:security_level strict
    assert_output --partial "NOPERM"
}

@test "valkey: dlp-reader ACL is restricted (read-only level)" {
    local dlp_pass=$(cat "${PROJECT_ROOT}/secrets/valkey_dlp_password.txt")
    
    # Verify dlp-reader can GET security level
    run docker exec "${VALKEY_CONTAINER}" valkey-cli --tls --cert /etc/valkey/tls/client.crt --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt --user dlp-reader --pass "$dlp_pass" GET polis:config:security_level
    assert_success
    
    # Verify dlp-reader cannot SET security level
    run docker exec "${VALKEY_CONTAINER}" valkey-cli --tls --cert /etc/valkey/tls/client.crt --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt --user dlp-reader --pass "$dlp_pass" SET polis:config:security_level relaxed
    assert_output --partial "NOPERM"
    
    # Verify dlp-reader cannot access other keys
    run docker exec "${VALKEY_CONTAINER}" valkey-cli --tls --cert /etc/valkey/tls/client.crt --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt --user dlp-reader --pass "$dlp_pass" GET polis:blocked:somekey
    assert_output --partial "NOPERM"
}
