#!/usr/bin/env bats
# bats file_tags=integration,security
# Integration tests for init container hardening — scanner-init, state-init

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    export SCANNER_INIT_INSPECT="$(docker inspect "$CTR_SCANNER_INIT" 2>/dev/null || echo '[]')"
    export STATE_INIT_INSPECT="$(docker inspect "$CTR_STATE_INIT" 2>/dev/null || echo '[]')"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_init_container "$CTR_SCANNER_INIT" "$CTR_STATE_INIT"
}

_inspect() { local var="${1//-/_}_INSPECT"; echo "${!var}"; }

# ── scanner-init hardening (source: docker-compose.yml) ───────────────────

@test "scanner-init: drops ALL capabilities" {
    run jq -r '.[0].HostConfig.CapDrop[]' <<< "$SCANNER_INIT_INSPECT"
    assert_success
    assert_output --partial "ALL"
}

@test "scanner-init: has CHOWN and DAC_OVERRIDE capabilities" {
    run jq -r '.[0].HostConfig.CapAdd[]' <<< "$SCANNER_INIT_INSPECT"
    assert_success
    assert_output --partial "CHOWN"
    assert_output --partial "DAC_OVERRIDE"
}

@test "scanner-init: has read-only rootfs" {
    run jq -r '.[0].HostConfig.ReadonlyRootfs' <<< "$SCANNER_INIT_INSPECT"
    assert_output "true"
}

@test "scanner-init: has no-new-privileges" {
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$SCANNER_INIT_INSPECT"
    assert_output --partial "no-new-privileges"
}

@test "scanner-init: memory limit is 32M" {
    run jq -r '.[0].HostConfig.Memory' <<< "$SCANNER_INIT_INSPECT"
    assert_output "33554432"
}

# ── state-init hardening (source: docker-compose.yml) ─────────────────────

@test "state-init: drops ALL capabilities" {
    run jq -r '.[0].HostConfig.CapDrop[]' <<< "$STATE_INIT_INSPECT"
    assert_success
    assert_output --partial "ALL"
}

@test "state-init: has only CHOWN capability" {
    run jq -r '.[0].HostConfig.CapAdd[]' <<< "$STATE_INIT_INSPECT"
    assert_success
    assert_output --regexp "^(CHOWN|CAP_CHOWN)$"
}

@test "state-init: has read-only rootfs" {
    run jq -r '.[0].HostConfig.ReadonlyRootfs' <<< "$STATE_INIT_INSPECT"
    assert_output "true"
}

@test "state-init: has no-new-privileges" {
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$STATE_INIT_INSPECT"
    assert_output --partial "no-new-privileges"
}

@test "state-init: memory limit is 32M" {
    run jq -r '.[0].HostConfig.Memory' <<< "$STATE_INIT_INSPECT"
    assert_output "33554432"
}
