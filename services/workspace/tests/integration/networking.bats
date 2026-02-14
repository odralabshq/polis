#!/usr/bin/env bats
# bats file_tags=integration,workspace
# Workspace Networking Integration Tests

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$WORKSPACE_CONTAINER"
}

@test "network: workspace only on internal-bridge" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output --partial "internal-bridge"
    refute_output --partial "gateway-bridge"
    refute_output --partial "external-bridge"
}

@test "network: workspace cannot reach icap directly" {
    run docker exec "${WORKSPACE_CONTAINER}" timeout 2 bash -c "echo > /dev/tcp/icap/1344" 2>/dev/null
    assert_failure
}

@test "network: workspace can resolve gateway via DNS" {
    run docker exec "${WORKSPACE_CONTAINER}" getent hosts gate
    assert_success
}

@test "network: workspace has default route" {
    run docker exec "${WORKSPACE_CONTAINER}" ip route show default
    assert_success
}
