#!/usr/bin/env bats
# bats file_tags=unit,docker
# Hadolint Dockerfile validation

setup() {
    load "../../lib/test_helper.bash"
    SERVICES_DIR="$PROJECT_ROOT/services"
}

# ── Helper ────────────────────────────────────────────────────────────────

skip_if_no_hadolint() {
    command -v hadolint >/dev/null 2>&1 || skip "hadolint not installed"
}

run_hadolint() {
    local dockerfile="$1"
    # Ignore DL3008 (pin versions) - we use distro packages
    # Ignore DL3018 (apk pin versions) - same reason
    hadolint --ignore DL3008 --ignore DL3018 "$dockerfile"
}

# ── Gate ──────────────────────────────────────────────────────────────────

@test "hadolint: gate/Dockerfile passes" {
    skip_if_no_hadolint
    run run_hadolint "$SERVICES_DIR/gate/Dockerfile"
    assert_success
}

# ── Sentinel ──────────────────────────────────────────────────────────────

@test "hadolint: sentinel/Dockerfile passes" {
    skip_if_no_hadolint
    run run_hadolint "$SERVICES_DIR/sentinel/Dockerfile"
    assert_success
}

# ── Resolver ──────────────────────────────────────────────────────────────

@test "hadolint: resolver/Dockerfile passes" {
    skip_if_no_hadolint
    run run_hadolint "$SERVICES_DIR/resolver/Dockerfile"
    assert_success
}

# ── Toolbox ───────────────────────────────────────────────────────────────

@test "hadolint: toolbox/Dockerfile passes" {
    skip_if_no_hadolint
    run run_hadolint "$SERVICES_DIR/toolbox/Dockerfile"
    assert_success
}

# ── Workspace ─────────────────────────────────────────────────────────────

@test "hadolint: workspace/Dockerfile passes" {
    skip_if_no_hadolint
    run run_hadolint "$SERVICES_DIR/workspace/Dockerfile"
    assert_success
}

# ── Scanner ───────────────────────────────────────────────────────────────

@test "hadolint: scanner/Dockerfile passes" {
    skip_if_no_hadolint
    [ -f "$SERVICES_DIR/scanner/Dockerfile" ] || skip "scanner/Dockerfile not found"
    run run_hadolint "$SERVICES_DIR/scanner/Dockerfile"
    assert_success
}

# ── Builders ──────────────────────────────────────────────────────────────

@test "hadolint: _builders/g3/Dockerfile passes" {
    skip_if_no_hadolint
    run run_hadolint "$SERVICES_DIR/_builders/g3/Dockerfile"
    assert_success
}
