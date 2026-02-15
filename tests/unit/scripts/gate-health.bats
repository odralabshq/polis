#!/usr/bin/env bats
# bats file_tags=unit,scripts
# Gate health check script validation
# Source: services/gate/scripts/health.sh

setup() {
    load "../../lib/test_helper.bash"
    SCRIPT="$PROJECT_ROOT/services/gate/scripts/health.sh"
}

@test "gate health: health.sh exists and is executable" {
    [ -x "$SCRIPT" ]
}

@test "gate health: health.sh passes bash syntax check" {
    run bash -n "$SCRIPT"
    assert_success
}
