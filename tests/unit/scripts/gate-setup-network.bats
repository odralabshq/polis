#!/usr/bin/env bats
# bats file_tags=unit,scripts
# Gate network setup script validation

setup() {
    load "../../lib/test_helper.bash"
    SCRIPT="$PROJECT_ROOT/services/gate/scripts/setup-network.sh"
}

@test "gate setup-network: setup-network.sh exists and is executable" {
    [ -x "$SCRIPT" ]
}

@test "gate setup-network: setup-network.sh passes bash syntax check" {
    run bash -n "$SCRIPT"
    assert_success
}
