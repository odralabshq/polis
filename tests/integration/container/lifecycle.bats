#!/usr/bin/env bats
# bats file_tags=integration,container
# Integration tests for container lifecycle — exists, running, healthy, init containers

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    # Cache inspect data for all containers
    for ctr in "${ALL_CONTAINERS[@]}" "${ALL_INIT_CONTAINERS[@]}"; do
        local var="${ctr//-/_}_INSPECT"
        export "$var"="$(docker inspect "$ctr" 2>/dev/null || echo '[]')"
    done
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/assertions/container.bash"
}

# Helper to get cached inspect var
_inspect() { local var="${1//-/_}_INSPECT"; echo "${!var}"; }

# ── Container exists (7) ──────────────────────────────────────────────────

@test "resolver: container exists" {
    run jq -e '.[0].Id' <<< "$(_inspect "$CTR_RESOLVER")"
    assert_success
}

@test "gate: container exists" {
    run jq -e '.[0].Id' <<< "$(_inspect "$CTR_GATE")"
    assert_success
}

@test "certgen: container exists" {
    run jq -e '.[0].Id' <<< "$(_inspect "$CTR_CERTGEN")"
    assert_success
}

@test "sentinel: container exists" {
    run jq -e '.[0].Id' <<< "$(_inspect "$CTR_SENTINEL")"
    assert_success
}

@test "scanner: container exists" {
    run jq -e '.[0].Id' <<< "$(_inspect "$CTR_SCANNER")"
    assert_success
}

@test "state: container exists" {
    run jq -e '.[0].Id' <<< "$(_inspect "$CTR_STATE")"
    assert_success
}

@test "toolbox: container exists" {
    run jq -e '.[0].Id' <<< "$(_inspect "$CTR_TOOLBOX")"
    assert_success
}

@test "workspace: container exists" {
    run jq -e '.[0].Id' <<< "$(_inspect "$CTR_WORKSPACE")"
    assert_success
}

# ── Container running (7) ─────────────────────────────────────────────────

@test "resolver: container is running" {
    run jq -r '.[0].State.Status' <<< "$(_inspect "$CTR_RESOLVER")"
    assert_output "running"
}

@test "gate: container is running" {
    run jq -r '.[0].State.Status' <<< "$(_inspect "$CTR_GATE")"
    assert_output "running"
}

@test "certgen: container is running" {
    run jq -r '.[0].State.Status' <<< "$(_inspect "$CTR_CERTGEN")"
    assert_output "running"
}

@test "sentinel: container is running" {
    run jq -r '.[0].State.Status' <<< "$(_inspect "$CTR_SENTINEL")"
    assert_output "running"
}

@test "scanner: container is running" {
    run jq -r '.[0].State.Status' <<< "$(_inspect "$CTR_SCANNER")"
    assert_output "running"
}

@test "state: container is running" {
    run jq -r '.[0].State.Status' <<< "$(_inspect "$CTR_STATE")"
    assert_output "running"
}

@test "toolbox: container is running" {
    run jq -r '.[0].State.Status' <<< "$(_inspect "$CTR_TOOLBOX")"
    assert_output "running"
}

@test "workspace: container is running" {
    run jq -r '.[0].State.Status' <<< "$(_inspect "$CTR_WORKSPACE")"
    assert_output "running"
}

# ── Container healthy (7) ─────────────────────────────────────────────────

@test "resolver: container is healthy" {
    run jq -r '.[0].State.Health.Status' <<< "$(_inspect "$CTR_RESOLVER")"
    assert_output "healthy"
}

@test "gate: container is healthy" {
    run jq -r '.[0].State.Health.Status' <<< "$(_inspect "$CTR_GATE")"
    assert_output "healthy"
}

@test "certgen: container is healthy" {
    run jq -r '.[0].State.Health.Status' <<< "$(_inspect "$CTR_CERTGEN")"
    assert_output "healthy"
}

@test "sentinel: container is healthy" {
    run jq -r '.[0].State.Health.Status' <<< "$(_inspect "$CTR_SENTINEL")"
    assert_output "healthy"
}

@test "scanner: container is healthy" {
    run jq -r '.[0].State.Health.Status' <<< "$(_inspect "$CTR_SCANNER")"
    assert_output "healthy"
}

@test "state: container is healthy" {
    run jq -r '.[0].State.Health.Status' <<< "$(_inspect "$CTR_STATE")"
    assert_output "healthy"
}

@test "toolbox: container is healthy" {
    run jq -r '.[0].State.Health.Status' <<< "$(_inspect "$CTR_TOOLBOX")"
    assert_output "healthy"
}

@test "workspace: container is healthy" {
    run jq -r '.[0].State.Health.Status' <<< "$(_inspect "$CTR_WORKSPACE")"
    assert_output "healthy"
}

# ── Init containers (4) ───────────────────────────────────────────────────

@test "scanner-init: completed successfully" {
    run jq -r '.[0].State.ExitCode' <<< "$(_inspect "$CTR_SCANNER_INIT")"
    assert_output "0"
}

@test "state-init: completed successfully" {
    run jq -r '.[0].State.ExitCode' <<< "$(_inspect "$CTR_STATE_INIT")"
    assert_output "0"
}
