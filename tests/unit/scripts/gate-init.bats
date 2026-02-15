#!/usr/bin/env bats
# bats file_tags=unit,scripts
# Gate init script validation

setup() {
    load "../../lib/test_helper.bash"
    SCRIPT="$PROJECT_ROOT/services/gate/scripts/init.sh"
}

@test "gate init: init.sh exists and is executable" {
    [ -x "$SCRIPT" ]
}

@test "gate init: init.sh passes bash syntax check" {
    run bash -n "$SCRIPT"
    assert_success
}
