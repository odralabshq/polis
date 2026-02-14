#!/usr/bin/env bats
# bats file_tags=integration,workspace
# Workspace Configuration Integration Tests

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$WORKSPACE_CONTAINER"
}

@test "config: polis-init.service type is oneshot" {
    run docker exec "${WORKSPACE_CONTAINER}" cat /etc/systemd/system/polis-init.service
    assert_success
    assert_output --partial "Type=oneshot"
}

@test "config: workspace init script mounted" {
    run docker inspect "${WORKSPACE_CONTAINER}" --format '{{json .Mounts}}'
    assert_success
    assert_output --partial "services/workspace/scripts/init.sh"
}
