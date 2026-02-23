#!/usr/bin/env bats
# bats file_tags=unit,scripts
# State health check input validation (no Docker needed — tests exit before connect)
# Source: services/state/scripts/health.sh

setup() {
    load "../../lib/test_helper.bash"
    SCRIPT="$PROJECT_ROOT/services/state/scripts/health.sh"
}

@test "state health: invalid VALKEY_HOST rejected" {
    run env VALKEY_HOST='bad;host' VALKEY_PORT=6379 bash "$SCRIPT"
    assert_failure
    assert_output --partial "CRITICAL"
}

@test "state health: non-numeric VALKEY_PORT rejected" {
    run env VALKEY_HOST=valkey VALKEY_PORT=abc bash "$SCRIPT"
    assert_failure
    assert_output --partial "CRITICAL"
}

@test "state health: out-of-range VALKEY_PORT rejected" {
    run env VALKEY_HOST=valkey VALKEY_PORT=0 bash "$SCRIPT"
    assert_failure
    assert_output --partial "CRITICAL"
}
