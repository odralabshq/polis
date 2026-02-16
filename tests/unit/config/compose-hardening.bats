#!/usr/bin/env bats
# bats file_tags=unit,config
# Compose hardening validation — F-21 (RUST_LOG), F-12 (CHOWN), F-20 (sentinel seccomp)

setup() {
    load "../../lib/test_helper.bash"
    COMPOSE="$PROJECT_ROOT/docker-compose.yml"
}

@test "compose: no RUST_LOG=debug in any service" {
    run grep "RUST_LOG=debug" "$COMPOSE"
    assert_failure
}

@test "compose: sentinel has seccomp profile" {
    run grep -A1 "seccomp=.*sentinel.*seccomp.json" "$COMPOSE"
    assert_success
}

@test "compose: sentinel has no cap_add" {
    # Between sentinel's cap_drop and networks, there should be no cap_add
    run sed -n '/container_name: polis-sentinel/,/container_name:/p' "$COMPOSE"
    refute_output --partial "cap_add"
}

@test "compose: scanner has no cap_add" {
    run sed -n '/container_name: polis-scanner$/,/container_name:/p' "$COMPOSE"
    refute_output --partial "cap_add"
}
