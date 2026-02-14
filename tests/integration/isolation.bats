#!/usr/bin/env bats
# bats file_tags=integration,network
# Global Network Isolation Tests

setup() {
    load "../helpers/common.bash"
    require_container "$GATEWAY_CONTAINER" "$ICAP_CONTAINER" "$WORKSPACE_CONTAINER"
}

@test "isolation: icap cannot reach workspace directly" {
    local workspace_ip
    workspace_ip=$(docker inspect --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "${WORKSPACE_CONTAINER}")
    run docker exec "${ICAP_CONTAINER}" timeout 2 bash -c "echo > /dev/tcp/${workspace_ip}/22" 2>/dev/null
    assert_failure
}

@test "isolation: internal-bridge has IPv6 disabled" {
    run docker network inspect --format '{{.EnableIPv6}}' polis_internal-bridge
    assert_success
    assert_output "false"
}
