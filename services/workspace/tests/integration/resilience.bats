#!/usr/bin/env bats
# bats file_tags=integration,workspace
# Workspace Resilience Integration Tests

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$WORKSPACE_CONTAINER"
}

@test "resilience: workspace has healthcheck configured" {
    run docker inspect --format '{{.Config.Healthcheck.Test}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output --partial "systemctl"
}

@test "resilience: workspace uses json-file logging driver" {
    run docker inspect --format '{{.HostConfig.LogConfig.Type}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output "json-file"
}

@test "resilience: workspace restart policy is unless-stopped" {
    run docker inspect --format '{{.HostConfig.RestartPolicy.Name}}' "${WORKSPACE_CONTAINER}"
    assert_success
    assert_output "unless-stopped"
}

@test "resilience: workspace init updates CA certificates" {
    run docker exec "${WORKSPACE_CONTAINER}" cat /usr/local/bin/polis-init.sh
    assert_success
    assert_output --partial "update-ca-certificates"
}
