#!/usr/bin/env bats
# bats file_tags=unit,security
# Dockerfile hardening validation

setup() {
    load "../../lib/test_helper.bash"
    GATE_DOCKERFILE="$PROJECT_ROOT/services/gate/Dockerfile"
    SENTINEL_DOCKERFILE="$PROJECT_ROOT/services/sentinel/Dockerfile"
    RESOLVER_DOCKERFILE="$PROJECT_ROOT/services/resolver/Dockerfile"
    TOOLBOX_DOCKERFILE="$PROJECT_ROOT/services/toolbox/Dockerfile"
    WORKSPACE_DOCKERFILE="$PROJECT_ROOT/services/workspace/Dockerfile"
}

# ── Source verification ───────────────────────────────────────────────────

@test "dockerfile: gate has SHA256 verification" {
    run grep "sha256sum -c" "$GATE_DOCKERFILE"
    assert_success
}

@test "dockerfile: gate pins G3_SHA256 hash" {
    run grep "ENV G3_SHA256=" "$GATE_DOCKERFILE"
    assert_success
}

@test "dockerfile: sentinel has SHA256 verification" {
    run grep "sha256sum -c" "$SENTINEL_DOCKERFILE"
    assert_success
}

@test "dockerfile: gate creates non-root user" {
    run grep -E "gate:x:999" "$GATE_DOCKERFILE"
    assert_success
}

# ── Base images (public Docker Hub) ───────────────────────────────────────

@test "dockerfile: resolver uses golang build image" {
    run grep -E "^FROM golang:" "$RESOLVER_DOCKERFILE"
    assert_success
}

@test "dockerfile: resolver uses debian runtime image" {
    run grep -E "^FROM (debian:|alpine:)" "$RESOLVER_DOCKERFILE"
    assert_success
}

@test "dockerfile: toolbox uses rust build image" {
    run grep -E "^FROM rust:" "$TOOLBOX_DOCKERFILE"
    assert_success
}

@test "dockerfile: toolbox uses debian runtime image" {
    run grep -E "^FROM debian:" "$TOOLBOX_DOCKERFILE"
    assert_success
}

@test "dockerfile: sentinel uses debian base" {
    run grep -E "^FROM debian:" "$SENTINEL_DOCKERFILE"
    assert_success
}

@test "dockerfile: gate uses rust build image" {
    run grep -E "^FROM rust:" "$GATE_DOCKERFILE"
    assert_success
}

@test "dockerfile: gate uses debian runtime image" {
    run grep -E "^FROM debian:" "$GATE_DOCKERFILE"
    assert_success
}

@test "dockerfile: workspace uses debian base" {
    run grep -E "^FROM debian:" "$WORKSPACE_DOCKERFILE"
    assert_success
}

# ── Non-root user ─────────────────────────────────────────────────────────

@test "dockerfile: resolver runs as non-root" {
    run grep -E "^USER " "$RESOLVER_DOCKERFILE"
    assert_success
    refute_output --partial "USER root"
    refute_output --partial "USER 0"
}

@test "dockerfile: toolbox runs as non-root" {
    run grep -E "^USER toolbox" "$TOOLBOX_DOCKERFILE"
    assert_success
}

@test "dockerfile: sentinel runs as non-root" {
    run grep -E "^USER sentinel" "$SENTINEL_DOCKERFILE"
    assert_success
}

@test "dockerfile: gate runs as non-root" {
    run grep -E "^USER gate" "$GATE_DOCKERFILE"
    assert_success
}
