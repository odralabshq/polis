#!/usr/bin/env bats
# bats file_tags=integration,security
# Integration tests for privilege flags — privileged, no-new-privileges, read_only, seccomp

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    for ctr in "$CTR_GATE" "$CTR_SENTINEL" "$CTR_SCANNER" "$CTR_STATE" "$CTR_TOOLBOX" "$CTR_WORKSPACE" "$CTR_RESOLVER"; do
        local var="${ctr//-/_}_INSPECT"
        export "$var"="$(docker inspect "$ctr" 2>/dev/null || echo '[]')"
    done
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    load "../../lib/assertions/security.bash"
}

_inspect() { local var="${1//-/_}_INSPECT"; echo "${!var}"; }

# ── Not privileged (5) ────────────────────────────────────────────────────

@test "gate: is NOT privileged" {
    require_container "$CTR_GATE"
    run jq -r '.[0].HostConfig.Privileged' <<< "$(_inspect "$CTR_GATE")"
    assert_output "false"
}

@test "sentinel: is NOT privileged" {
    require_container "$CTR_SENTINEL"
    run jq -r '.[0].HostConfig.Privileged' <<< "$(_inspect "$CTR_SENTINEL")"
    assert_output "false"
}

@test "scanner: is NOT privileged" {
    require_container "$CTR_SCANNER"
    run jq -r '.[0].HostConfig.Privileged' <<< "$(_inspect "$CTR_SCANNER")"
    assert_output "false"
}

@test "state: is NOT privileged" {
    require_container "$CTR_STATE"
    run jq -r '.[0].HostConfig.Privileged' <<< "$(_inspect "$CTR_STATE")"
    assert_output "false"
}

@test "workspace: is NOT privileged" {
    require_container "$CTR_WORKSPACE"
    run jq -r '.[0].HostConfig.Privileged' <<< "$(_inspect "$CTR_WORKSPACE")"
    assert_output "false"
}

# ── No-new-privileges (5) — source: docker-compose.yml security_opt ──────

@test "gate: has no-new-privileges" {
    require_container "$CTR_GATE"
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$(_inspect "$CTR_GATE")"
    assert_output --partial "no-new-privileges"
}

@test "sentinel: has no-new-privileges" {
    require_container "$CTR_SENTINEL"
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$(_inspect "$CTR_SENTINEL")"
    assert_output --partial "no-new-privileges"
}

@test "scanner: has no-new-privileges" {
    require_container "$CTR_SCANNER"
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$(_inspect "$CTR_SCANNER")"
    assert_output --partial "no-new-privileges"
}

@test "state: has no-new-privileges" {
    require_container "$CTR_STATE"
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$(_inspect "$CTR_STATE")"
    assert_output --partial "no-new-privileges"
}

@test "toolbox: has no-new-privileges" {
    require_container "$CTR_TOOLBOX"
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$(_inspect "$CTR_TOOLBOX")"
    assert_output --partial "no-new-privileges"
}

# ── Read-only rootfs (2) — source: docker-compose.yml read_only: true ────

@test "gate: has read-only rootfs" {
    require_container "$CTR_GATE"
    run jq -r '.[0].HostConfig.ReadonlyRootfs' <<< "$(_inspect "$CTR_GATE")"
    assert_output "true"
}

@test "scanner: has read-only rootfs" {
    require_container "$CTR_SCANNER"
    run jq -r '.[0].HostConfig.ReadonlyRootfs' <<< "$(_inspect "$CTR_SCANNER")"
    assert_output "true"
}

@test "state: has read-only rootfs" {
    require_container "$CTR_STATE"
    run jq -r '.[0].HostConfig.ReadonlyRootfs' <<< "$(_inspect "$CTR_STATE")"
    assert_output "true"
}

@test "sentinel: has read-only rootfs" {
    require_container "$CTR_SENTINEL"
    run jq -r '.[0].HostConfig.ReadonlyRootfs' <<< "$(_inspect "$CTR_SENTINEL")"
    assert_output "true"
}

@test "toolbox: has read-only rootfs" {
    require_container "$CTR_TOOLBOX"
    run jq -r '.[0].HostConfig.ReadonlyRootfs' <<< "$(_inspect "$CTR_TOOLBOX")"
    assert_output "true"
}

# ── Seccomp profile applied — source: docker-compose.yml security_opt seccomp= ──

@test "gate: has seccomp profile applied" {
    require_container "$CTR_GATE"
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$(_inspect "$CTR_GATE")"
    assert_output --partial "seccomp="
}

@test "sentinel: has seccomp profile applied" {
    require_container "$CTR_SENTINEL"
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$(_inspect "$CTR_SENTINEL")"
    assert_output --partial "seccomp="
}

@test "scanner: has seccomp profile applied" {
    require_container "$CTR_SCANNER"
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$(_inspect "$CTR_SCANNER")"
    assert_output --partial "seccomp="
}

@test "toolbox: has seccomp profile applied" {
    require_container "$CTR_TOOLBOX"
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$(_inspect "$CTR_TOOLBOX")"
    assert_output --partial "seccomp="
}

@test "workspace: has seccomp profile applied" {
    require_container "$CTR_WORKSPACE"
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$(_inspect "$CTR_WORKSPACE")"
    assert_output --partial "seccomp="
}

# ── Resolver hardening (source: docker-compose.yml) ───────────────────────

@test "resolver: has read-only rootfs" {
    require_container "$CTR_RESOLVER"
    run jq -r '.[0].HostConfig.ReadonlyRootfs' <<< "$(_inspect "$CTR_RESOLVER")"
    assert_output "true"
}

@test "resolver: has seccomp profile applied" {
    require_container "$CTR_RESOLVER"
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$(_inspect "$CTR_RESOLVER")"
    assert_output --partial "seccomp="
}
