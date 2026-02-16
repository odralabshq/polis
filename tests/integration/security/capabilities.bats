#!/usr/bin/env bats
# bats file_tags=integration,security
# Integration tests for Linux capabilities — cap_drop/cap_add per container

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

# ── Gate capabilities (source: docker-compose.yml cap_drop:[ALL] cap_add:[NET_ADMIN,NET_RAW,SETUID,SETGID]) ──

@test "gate: drops ALL capabilities" {
    require_container "$CTR_GATE"
    run jq -r '.[0].HostConfig.CapDrop[]' <<< "$(_inspect "$CTR_GATE")"
    assert_success
    assert_output --partial "ALL"
}

@test "gate: has NET_ADMIN capability" {
    require_container "$CTR_GATE"
    run jq -r '.[0].HostConfig.CapAdd[]' <<< "$(_inspect "$CTR_GATE")"
    assert_success
    assert_output --partial "NET_ADMIN"
}

@test "gate: has NET_RAW capability" {
    require_container "$CTR_GATE"
    run jq -r '.[0].HostConfig.CapAdd[]' <<< "$(_inspect "$CTR_GATE")"
    assert_success
    assert_output --partial "NET_RAW"
}

@test "gate: effective capabilities are restricted" {
    require_container "$CTR_GATE"
    run docker exec "$CTR_GATE" grep '^CapEff:' /proc/1/status
    assert_success
    local capeff="${output##*:}"
    capeff="${capeff// /}"
    # Must not be full capability set (000001ffffffffff or similar)
    [[ "$capeff" != "000001ffffffffff" ]] || fail "Gate has full capability set: $capeff"
    [[ "$capeff" != "0000000000000000" ]] || fail "Gate has no capabilities: $capeff"
}

# ── Sentinel capabilities (source: docker-compose.yml cap_drop:[ALL]) ──

@test "sentinel: drops ALL capabilities" {
    require_container "$CTR_SENTINEL"
    run jq -r '.[0].HostConfig.CapDrop[]' <<< "$(_inspect "$CTR_SENTINEL")"
    assert_success
    assert_output --partial "ALL"
}

@test "sentinel: does NOT have CHOWN capability" {
    require_container "$CTR_SENTINEL"
    run jq -r '.[0].HostConfig.CapAdd // [] | .[]' <<< "$(_inspect "$CTR_SENTINEL")"
    refute_output --partial "CHOWN"
}

@test "sentinel: does NOT have SETGID capability" {
    require_container "$CTR_SENTINEL"
    run jq -r '.[0].HostConfig.CapAdd // [] | .[]' <<< "$(_inspect "$CTR_SENTINEL")"
    refute_output --partial "SETGID"
}

# ── Scanner capabilities (source: docker-compose.yml cap_drop:[ALL]) ──

@test "scanner: drops ALL capabilities" {
    require_container "$CTR_SCANNER"
    run jq -r '.[0].HostConfig.CapDrop[]' <<< "$(_inspect "$CTR_SCANNER")"
    assert_success
    assert_output --partial "ALL"
}

@test "scanner: does NOT have CHOWN capability" {
    require_container "$CTR_SCANNER"
    run jq -r '.[0].HostConfig.CapAdd // [] | .[]' <<< "$(_inspect "$CTR_SCANNER")"
    refute_output --partial "CHOWN"
}

# ── State / Toolbox / Workspace: drop ALL, no cap_add ─────────────────────

@test "state: drops ALL capabilities" {
    require_container "$CTR_STATE"
    run jq -r '.[0].HostConfig.CapDrop[]' <<< "$(_inspect "$CTR_STATE")"
    assert_success
    assert_output --partial "ALL"
}

@test "toolbox: drops ALL capabilities" {
    require_container "$CTR_TOOLBOX"
    run jq -r '.[0].HostConfig.CapDrop[]' <<< "$(_inspect "$CTR_TOOLBOX")"
    assert_success
    assert_output --partial "ALL"
}

@test "workspace: drops ALL capabilities" {
    require_container "$CTR_WORKSPACE"
    run jq -r '.[0].HostConfig.CapDrop[]' <<< "$(_inspect "$CTR_WORKSPACE")"
    assert_success
    assert_output --partial "ALL"
}

# ── Resolver capabilities (source: docker-compose.yml cap_drop:[ALL]) ─────

@test "resolver: drops ALL capabilities" {
    require_container "$CTR_RESOLVER"
    run jq -r '.[0].HostConfig.CapDrop[]' <<< "$(_inspect "$CTR_RESOLVER")"
    assert_success
    assert_output --partial "ALL"
}
