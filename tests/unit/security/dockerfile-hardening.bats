#!/usr/bin/env bats
# bats file_tags=unit,security
# Dockerfile hardening validation

setup() {
    load "../../lib/test_helper.bash"
    GATE_DOCKERFILE="$PROJECT_ROOT/services/gate/Dockerfile"
    SENTINEL_DOCKERFILE="$PROJECT_ROOT/services/sentinel/Dockerfile"
}

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
    run grep "useradd" "$GATE_DOCKERFILE"
    assert_success
}
