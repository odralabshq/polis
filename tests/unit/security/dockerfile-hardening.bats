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
    SCANNER_DOCKERFILE="$PROJECT_ROOT/services/scanner/Dockerfile"
    CERTGEN_DOCKERFILE="$PROJECT_ROOT/services/certgen/Dockerfile"
    G3_BUILDER_DOCKERFILE="$PROJECT_ROOT/services/_builders/g3/Dockerfile"
}

# ── Source verification ───────────────────────────────────────────────────

@test "dockerfile: g3-builder has SHA256 verification" {
    run grep "sha256sum -c" "$G3_BUILDER_DOCKERFILE"
    assert_success
}

@test "dockerfile: g3-builder pins G3_SHA256 hash" {
    run grep "ENV G3_SHA256=" "$G3_BUILDER_DOCKERFILE"
    assert_success
}

@test "dockerfile: sentinel has SHA256 verification" {
    run grep "sha256sum -c" "$SENTINEL_DOCKERFILE"
    assert_success
}

# ── Base images (DHI private registry) ────────────────────────────────────

@test "dockerfile: resolver uses DHI golang build image" {
    run grep -E "^FROM dhi\\.io/golang:" "$RESOLVER_DOCKERFILE"
    assert_success
}

@test "dockerfile: resolver uses DHI debian runtime image" {
    run grep -E "^FROM dhi\\.io/debian-base:" "$RESOLVER_DOCKERFILE"
    assert_success
}

@test "dockerfile: toolbox uses DHI rust build image" {
    run grep -E "^FROM dhi\\.io/rust:" "$TOOLBOX_DOCKERFILE"
    assert_success
}

@test "dockerfile: toolbox uses DHI debian runtime image" {
    run grep -E "^FROM dhi\\.io/debian-base:" "$TOOLBOX_DOCKERFILE"
    assert_success
}

@test "dockerfile: sentinel uses DHI debian base" {
    run grep -E "^FROM dhi\\.io/debian-base:" "$SENTINEL_DOCKERFILE"
    assert_success
}

@test "dockerfile: gate uses shared g3-builder image" {
    run grep -E "^FROM ghcr\\.io/odralabshq/g3-builder:" "$GATE_DOCKERFILE"
    assert_success
}

@test "dockerfile: g3-builder uses DHI rust build image with digest" {
    run grep -E "^FROM dhi\.io/rust.*@sha256:" "$G3_BUILDER_DOCKERFILE"
    assert_success
}

@test "dockerfile: workspace uses DHI debian base" {
    run grep -E "^FROM dhi\\.io/debian-base:" "$WORKSPACE_DOCKERFILE"
    assert_success
}

@test "dockerfile: scanner uses DHI clamav image" {
    run grep -E "^FROM dhi\\.io/clamav:" "$SCANNER_DOCKERFILE"
    assert_success
}

@test "dockerfile: certgen uses shared g3-builder image" {
    run grep -E "^FROM ghcr\\.io/odralabshq/g3-builder:" "$CERTGEN_DOCKERFILE"
    assert_success
}

@test "dockerfile: certgen uses DHI debian runtime image" {
    run grep -E "^FROM dhi\\.io/debian-base:" "$CERTGEN_DOCKERFILE"
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
    run grep -E "^USER nonroot" "$TOOLBOX_DOCKERFILE"
    assert_success
}

@test "dockerfile: sentinel runs as non-root" {
    run grep -E "^USER nonroot" "$SENTINEL_DOCKERFILE"
    assert_success
}

@test "dockerfile: gate runs as non-root" {
    run grep -E "^USER nonroot" "$GATE_DOCKERFILE"
    assert_success
}
