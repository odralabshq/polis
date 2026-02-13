#!/usr/bin/env bats
# Gate Resilience Integration Tests

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$GATEWAY_CONTAINER"
}

# =============================================================================
# Health Check Tests
# =============================================================================

@test "resilience: gateway has healthcheck configured" {
    run docker inspect --format '{{.Config.Healthcheck.Test}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output --partial "health-check.sh"
}

@test "resilience: health-check.sh mounted in gateway" {
    run docker exec "${GATEWAY_CONTAINER}" test -f /scripts/health-check.sh
    assert_success
}

@test "resilience: gateway currently healthy" {
    run docker inspect --format '{{.State.Health.Status}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "healthy"
}

# =============================================================================
# Logging & Restart Policy
# =============================================================================

@test "resilience: gateway uses json-file logging driver" {
    run docker inspect --format '{{.HostConfig.LogConfig.Type}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "json-file"
}

@test "resilience: gateway restart policy is unless-stopped" {
    run docker inspect --format '{{.HostConfig.RestartPolicy.Name}}' "${GATEWAY_CONTAINER}"
    assert_success
    assert_output "unless-stopped"
}

# =============================================================================
# Init Script Verification
# =============================================================================

@test "resilience: gateway init starts g3fcgen before g3proxy" {
    run docker exec "${GATEWAY_CONTAINER}" cat /init.sh
    assert_success
    local g3fcgen_line=$(docker exec "${GATEWAY_CONTAINER}" grep -n "g3fcgen" /init.sh | head -1 | cut -d: -f1)
    local g3proxy_line=$(docker exec "${GATEWAY_CONTAINER}" grep -n "g3proxy.yaml" /init.sh | cut -d: -f1)
    [[ "$g3fcgen_line" -lt "$g3proxy_line" ]]
}
