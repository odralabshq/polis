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

@test "gate: g3proxy runs as gate user (999)" {
    require_container "$CTR_GATE"
    run bash -c "docker top $CTR_GATE | grep g3proxy | awk '{print \$1}'"
    assert_success
    assert_output --partial "999"
}

@test "sentinel: c-icap runs as nonroot (65532)" {
    require_container "$CTR_SENTINEL"
    run bash -c "docker top $CTR_SENTINEL | grep c-icap | awk '{print \$1}'"
    assert_success
    assert_output --partial "65532"
}

# ── Container UIDs (source: docker-compose.yml user: "UID:GID") ──────────

@test "scanner: ClamAV manages its own user (clamav)" {
    require_container "$CTR_SCANNER"
    # ClamAV image runs as root initially, then drops to clamav user internally
    # On clamav/clamav image, the clamav user maps to UID 100
    run bash -c "docker top $CTR_SCANNER | grep -E 'clamd|freshclam' | head -1"
    assert_success
    # Process should NOT be running as root
    refute_output --regexp "^root "
}

@test "resolver: runs as UID 200 (resolver)" {
    require_container "$CTR_RESOLVER"
    run docker exec "$CTR_RESOLVER" id -u
    assert_success
    assert_output "200"
}

@test "state: runs as UID 999 (valkey)" {
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
