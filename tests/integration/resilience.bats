#!/usr/bin/env bats
# =============================================================================
# Resilience & Observability Tests
# Tests for: health checks, fail-closed ICAP, JSON logging, cert validation
# Issue: 03-resilience-observability
# =============================================================================

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
# Health Check Configuration Tests
# =============================================================================

@test "resilience: gateway has healthcheck configured" {
    run docker inspect --format '{{.Config.Healthcheck.Test}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output --partial "health-check.sh"
}

@test "resilience: gateway healthcheck interval is 10s" {
    run docker inspect --format '{{.Config.Healthcheck.Interval}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "10s"
}

@test "resilience: gateway healthcheck timeout is 5s" {
    run docker inspect --format '{{.Config.Healthcheck.Timeout}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "5s"
}

@test "resilience: gateway healthcheck retries is 3" {
    run docker inspect --format '{{.Config.Healthcheck.Retries}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "3"
}

@test "resilience: icap has healthcheck configured" {
    run docker inspect --format '{{.Config.Healthcheck.Test}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output --partial "pgrep"
}

@test "resilience: workspace has healthcheck configured" {
    run docker inspect --format '{{.Config.Healthcheck.Test}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output --partial "systemctl"
}

# =============================================================================
# JSON Logging Tests
# =============================================================================

@test "resilience: gateway uses json-file logging driver" {
    run docker inspect --format '{{.HostConfig.LogConfig.Type}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "json-file"
}

@test "resilience: gateway log max-size is 50m" {
    run docker inspect --format '{{index .HostConfig.LogConfig.Config "max-size"}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "50m"
}

@test "resilience: gateway log max-file is 5" {
    run docker inspect --format '{{index .HostConfig.LogConfig.Config "max-file"}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "5"
}

@test "resilience: icap uses json-file logging driver" {
    run docker inspect --format '{{.HostConfig.LogConfig.Type}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output "json-file"
}

@test "resilience: workspace uses json-file logging driver" {
    run docker inspect --format '{{.HostConfig.LogConfig.Type}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output "json-file"
}

@test "resilience: workspace log max-size is 100m" {
    run docker inspect --format '{{index .HostConfig.LogConfig.Config "max-size"}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output "100m"
}

# =============================================================================
# Service Labels Tests
# =============================================================================

@test "resilience: gateway has service label" {
    run docker inspect --format '{{index .Config.Labels "service"}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "polis-gateway"
}

@test "resilience: icap has service label" {
    run docker inspect --format '{{index .Config.Labels "service"}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output "polis-icap"
}

@test "resilience: workspace has service label" {
    run docker inspect --format '{{index .Config.Labels "service"}}' "${WORKSPACE_CONTAINER}"
    assert_success
    # Accept both base and openclaw profile labels
    assert_output --regexp "^polis-workspace(-openclaw)?$"
}

# =============================================================================
# Restart Policy Tests
# =============================================================================

@test "resilience: gateway restart policy is unless-stopped" {
    run docker inspect --format '{{.HostConfig.RestartPolicy.Name}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "unless-stopped"
}

@test "resilience: icap restart policy is unless-stopped" {
    run docker inspect --format '{{.HostConfig.RestartPolicy.Name}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output "unless-stopped"
}

@test "resilience: workspace restart policy is unless-stopped" {
    run docker inspect --format '{{.HostConfig.RestartPolicy.Name}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output "unless-stopped"
}

# =============================================================================
# Health Check Script Tests
# =============================================================================

@test "resilience: health-check.sh mounted in gateway" {
    run docker exec "${GATEWAY_CONTAINER}" test -f /scripts/health-check.sh
    assert_success
}

@test "resilience: health-check.sh is executable" {
    run docker exec "${GATEWAY_CONTAINER}" test -x /scripts/health-check.sh
    assert_success
}

@test "resilience: health-check.sh checks g3proxy process" {
    run docker exec "${GATEWAY_CONTAINER}" grep -q "pgrep -x g3proxy" /scripts/health-check.sh
    assert_success
}

@test "resilience: health-check.sh checks g3fcgen process" {
    run docker exec "${GATEWAY_CONTAINER}" grep -q "pgrep -x g3fcgen" /scripts/health-check.sh
    assert_success
}

@test "resilience: health-check.sh checks TPROXY rules" {
    run docker exec "${GATEWAY_CONTAINER}" grep -q "G3TPROXY" /scripts/health-check.sh
    assert_success
}

@test "resilience: health-check.sh checks ICAP connectivity" {
    run docker exec "${GATEWAY_CONTAINER}" grep -q "icap:1344" /scripts/health-check.sh
    assert_success
}

# =============================================================================
# Certificate Validation Tests
# =============================================================================

@test "resilience: init script has validate_certificates function" {
    run docker exec "${GATEWAY_CONTAINER}" grep -q "validate_certificates" /init.sh
    assert_success
}

@test "resilience: cert validation checks expiry" {
    run docker exec "${GATEWAY_CONTAINER}" grep -q "checkend 86400" /init.sh
    assert_success
}

@test "resilience: cert validation uses SHA-256" {
    run docker exec "${GATEWAY_CONTAINER}" grep -q "openssl sha256" /init.sh
    assert_success
}

@test "resilience: cert validation checks cert/key match" {
    run docker exec "${GATEWAY_CONTAINER}" grep -q "cert_modulus.*key_modulus" /init.sh
    assert_success
}

# =============================================================================
# Fail-Closed Behavior Tests (ICAP dependency)
# =============================================================================

@test "resilience: gateway health check verifies ICAP reachability" {
    # Health check should include ICAP connectivity test
    run docker exec "${GATEWAY_CONTAINER}" cat /scripts/health-check.sh
    assert_success
    assert_output --partial "icap:1344"
}

@test "resilience: gateway currently healthy with ICAP running" {
    run docker inspect --format '{{.State.Health.Status}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "healthy"
}

@test "resilience: icap currently healthy" {
    run docker inspect --format '{{.State.Health.Status}}' "${ICAP_CONTAINER}"
    assert_success
    assert_output "healthy"
}

# =============================================================================
# Init Script Verification Tests
# =============================================================================

@test "resilience: gateway init starts g3fcgen before g3proxy" {
    # g3fcgen must start first for certificate generation
    run docker exec "${GATEWAY_CONTAINER}" cat /init.sh
    assert_success
    # g3fcgen line should appear before g3proxy exec (exec uses setpriv wrapper)
    local g3fcgen_line=$(docker exec "${GATEWAY_CONTAINER}" grep -n "g3fcgen" /init.sh | head -1 | cut -d: -f1)
    local g3proxy_line=$(docker exec "${GATEWAY_CONTAINER}" grep -n "g3proxy.yaml" /init.sh | cut -d: -f1)
    [[ "$g3fcgen_line" -lt "$g3proxy_line" ]]
}

@test "resilience: gateway init waits for ICAP service" {
    run docker exec "${GATEWAY_CONTAINER}" grep -q "Waiting for ICAP" /init.sh
    assert_success
}

@test "resilience: gateway init cleans stale control sockets" {
    run docker exec "${GATEWAY_CONTAINER}" grep -q "rm -rf /tmp/g3" /init.sh
    assert_success
}

@test "resilience: gateway init creates control directory" {
    # Directory is created in Dockerfile, init script cleans stale contents
    run docker exec "${GATEWAY_CONTAINER}" test -d /tmp/g3
    assert_success
}

@test "resilience: workspace init updates CA certificates" {
    run docker exec "${WORKSPACE_CONTAINER}" cat /usr/local/bin/polis-init.sh
    assert_success
    assert_output --partial "update-ca-certificates"
}

@test "resilience: workspace init has WSL2 detection" {
    run docker exec "${WORKSPACE_CONTAINER}" cat /usr/local/bin/polis-init.sh
    assert_success
    assert_output --partial "is_wsl2"
}

@test "resilience: workspace init has fail-closed IPv6 check" {
    run docker exec "${WORKSPACE_CONTAINER}" cat /usr/local/bin/polis-init.sh
    assert_success
    assert_output --partial "CRITICAL"
    assert_output --partial "TPROXY bypass"
}

# =============================================================================
# Regression Tests - Workspace Routing (DO NOT REMOVE)
# =============================================================================

@test "resilience: workspace init sets default route via gateway" {
    run docker exec "${WORKSPACE_CONTAINER}" cat /usr/local/bin/polis-init.sh
    assert_success
    assert_output --partial "ip route del default"
    assert_output --partial "ip route add default via"
}

@test "resilience: workspace init resolves gateway via DNS" {
    run docker exec "${WORKSPACE_CONTAINER}" cat /usr/local/bin/polis-init.sh
    assert_success
    assert_output --partial "getent hosts gateway"
}

@test "resilience: polis-init.service is enabled via symlink" {
    # CRITICAL: Service must be enabled via symlink in Dockerfile, not file mount
    run docker exec "${WORKSPACE_CONTAINER}" readlink /etc/systemd/system/multi-user.target.wants/polis-init.service
    assert_success
    assert_output "/etc/systemd/system/polis-init.service"
}
