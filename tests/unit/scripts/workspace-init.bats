#!/usr/bin/env bats
# bats file_tags=unit,scripts
# Workspace init script validation

setup() {
    load "../../lib/test_helper.bash"
    SCRIPT="$PROJECT_ROOT/services/workspace/scripts/init.sh"
}

@test "workspace init: init.sh exists and is executable" {
    [ -x "$SCRIPT" ]
}

@test "workspace init: init.sh passes bash syntax check" {
    run bash -n "$SCRIPT"
    assert_success
}
