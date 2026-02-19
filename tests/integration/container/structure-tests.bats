#!/usr/bin/env bats
# bats file_tags=integration,container
# Container structure tests - validates built images

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    STRUCTURE_TESTS_DIR="$PROJECT_ROOT/tests/container-structure"
}

# ── Helper ────────────────────────────────────────────────────────────────

skip_if_no_cst() {
    command -v container-structure-test >/dev/null 2>&1 || skip "container-structure-test not installed"
}

skip_if_image_missing() {
    local image="$1"
    docker image inspect "$image" >/dev/null 2>&1 || skip "Image $image not found"
}

run_structure_test() {
    local image="$1"
    local config="$2"
    container-structure-test test --image "$image" --config "$config"
}

# ── Gate ──────────────────────────────────────────────────────────────────

@test "structure-test: gate image" {
    skip_if_no_cst
    skip_if_image_missing "polis-gate-oss:latest"
    run run_structure_test "polis-gate-oss:latest" "$STRUCTURE_TESTS_DIR/gate.yaml"
    assert_success
}

# ── Sentinel ──────────────────────────────────────────────────────────────

@test "structure-test: sentinel image" {
    skip_if_no_cst
    skip_if_image_missing "polis-sentinel-oss:latest"
    run run_structure_test "polis-sentinel-oss:latest" "$STRUCTURE_TESTS_DIR/sentinel.yaml"
    assert_success
}

# ── Resolver ──────────────────────────────────────────────────────────────

@test "structure-test: resolver image" {
    skip_if_no_cst
    skip_if_image_missing "polis-resolver-oss:latest"
    run run_structure_test "polis-resolver-oss:latest" "$STRUCTURE_TESTS_DIR/resolver.yaml"
    assert_success
}

# ── Toolbox ───────────────────────────────────────────────────────────────

@test "structure-test: toolbox image" {
    skip_if_no_cst
    skip_if_image_missing "polis-toolbox-oss:latest"
    run run_structure_test "polis-toolbox-oss:latest" "$STRUCTURE_TESTS_DIR/toolbox.yaml"
    assert_success
}

# ── Workspace ─────────────────────────────────────────────────────────────

@test "structure-test: workspace image" {
    skip_if_no_cst
    skip_if_image_missing "polis-workspace-oss:latest"
    run run_structure_test "polis-workspace-oss:latest" "$STRUCTURE_TESTS_DIR/workspace.yaml"
    assert_success
}

# ── Scanner ───────────────────────────────────────────────────────────────

@test "structure-test: scanner image" {
    skip_if_no_cst
    skip_if_image_missing "polis-scanner-oss:latest"
    run run_structure_test "polis-scanner-oss:latest" "$STRUCTURE_TESTS_DIR/scanner.yaml"
    assert_success
}

# ── Certgen ───────────────────────────────────────────────────────────────

@test "structure-test: certgen image" {
    skip_if_no_cst
    skip_if_image_missing "polis-certgen-oss:latest"
    run run_structure_test "polis-certgen-oss:latest" "$STRUCTURE_TESTS_DIR/certgen.yaml"
    assert_success
}
