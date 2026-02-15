#!/usr/bin/env bats
# bats file_tags=unit,scripts
# Scanner init script validation
# Note: uses #!/sbin/tini /bin/sh — syntax check with sh

setup() {
    load "../../lib/test_helper.bash"
    SCRIPT="$PROJECT_ROOT/services/scanner/scripts/init.sh"
}

@test "scanner init: init.sh exists and is executable" {
    [ -x "$SCRIPT" ]
}

@test "scanner init: init.sh passes shell syntax check" {
    run sh -n "$SCRIPT"
    assert_success
}
