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

@test "dockerfile: gate creates non-root user" {
    # DHI base images have nonroot user; we ensure it exists after apt-get
    run grep -E "useradd|nonroot.*65532" "$GATE_DOCKERFILE"
    assert_success
}

# ── DHI base images (Issues 15, 16) ───────────────────────────────────────

@test "dockerfile: resolver uses DHI golang build image with digest" {
    run grep -E "^FROM dhi\.io/golang.*@sha256:" "$RESOLVER_DOCKERFILE"
    assert_success
}

@test "dockerfile: resolver uses DHI static runtime with digest" {
    run grep -E "^FROM dhi\.io/static@sha256:" "$RESOLVER_DOCKERFILE"
    assert_success
}

@test "dockerfile: toolbox uses DHI rust build image with digest" {
    run grep -E "^FROM dhi\.io/rust.*@sha256:" "$TOOLBOX_DOCKERFILE"
    assert_success
}

@test "dockerfile: toolbox uses DHI debian-base runtime with digest" {
    run grep -E "^FROM dhi\.io/debian-base.*@sha256:" "$TOOLBOX_DOCKERFILE"
    assert_success
}

@test "dockerfile: sentinel uses DHI debian-base with digest" {
    run grep -E "^FROM dhi\.io/debian-base.*@sha256:" "$SENTINEL_DOCKERFILE"
    assert_success
}

@test "dockerfile: g3-builder uses DHI rust build image with digest" {
    run grep -E "^FROM dhi\.io/rust.*@sha256:" "$G3_BUILDER_DOCKERFILE"
    assert_success
}

@test "dockerfile: gate uses DHI debian-base runtime with digest" {
    run grep -E "^FROM dhi\.io/debian-base.*@sha256:" "$GATE_DOCKERFILE"
    assert_success
}

@test "dockerfile: workspace uses DHI debian-base with digest" {
    run grep -E "^FROM dhi\.io/debian-base.*@sha256:" "$WORKSPACE_DOCKERFILE"
    assert_success
}

# ── Nonroot user (UID 65532) ──────────────────────────────────────────────

@test "dockerfile: resolver runs as nonroot" {
    run grep -E "^USER (nonroot|65532)" "$RESOLVER_DOCKERFILE"
    assert_success
}

@test "dockerfile: toolbox runs as nonroot" {
    run grep -E "^USER (nonroot|65532)" "$TOOLBOX_DOCKERFILE"
    assert_success
}

@test "dockerfile: sentinel runs as nonroot" {
    run grep -E "^USER (nonroot|65532)" "$SENTINEL_DOCKERFILE"
    assert_success
}

@test "dockerfile: gate runs as root (NET_ADMIN required for TPROXY)" {
    run grep -E "^USER (root|0)" "$GATE_DOCKERFILE"
    assert_success
}
