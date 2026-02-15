#!/usr/bin/env bats
# bats file_tags=integration,security
# Integration tests for process users and UIDs per container

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
}

# ── Process users (source: docker-compose.yml user: fields) ───────────────

@test "gate: g3proxy runs as gate user" {
    require_container "$CTR_GATE"
    run docker exec "$CTR_GATE" ps -o user= -C g3proxy
    assert_success
    assert_output --partial "gate"
}

@test "sentinel: c-icap runs as sentinel user" {
    require_container "$CTR_SENTINEL"
    run docker exec "$CTR_SENTINEL" ps -o user= -C c-icap
    assert_success
    assert_output --partial "sentinel"
}

# ── Container UIDs (source: docker-compose.yml user: "UID:GID") ──────────

@test "scanner: runs as UID 100" {
    require_container "$CTR_SCANNER"
    run docker exec "$CTR_SCANNER" id -u
    assert_success
    assert_output "100"
}

@test "resolver: runs as UID 200" {
    require_container "$CTR_RESOLVER"
    run docker exec "$CTR_RESOLVER" id -u
    assert_success
    assert_output "200"
}

@test "state: runs as UID 999" {
    require_container "$CTR_STATE"
    run docker exec "$CTR_STATE" id -u
    assert_success
    assert_output "999"
}

# ── Workspace user setup ─────────────────────────────────────────────────

@test "workspace: polis user exists with UID 1000" {
    require_container "$CTR_WORKSPACE"
    run docker exec "$CTR_WORKSPACE" id -u polis
    assert_success
    assert_output "1000"
}

@test "workspace: root has nologin shell" {
    require_container "$CTR_WORKSPACE"
    run docker exec "$CTR_WORKSPACE" getent passwd root
    assert_success
    assert_output --regexp "(nologin|/bin/false)"
}
