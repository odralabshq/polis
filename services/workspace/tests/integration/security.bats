#!/usr/bin/env bats
# bats file_tags=integration,workspace
# Workspace Security Integration Tests

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$WORKSPACE_CONTAINER"
}

@test "security: workspace is NOT running privileged" {
    run docker inspect --format '{{.HostConfig.Privileged}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output "false"
}

@test "security: workspace uses sysbox runtime" {
    run docker inspect --format '{{.HostConfig.Runtime}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output "sysbox-runc"
}
