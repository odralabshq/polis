#!/usr/bin/env bats
# bats file_tags=integration,container
# Integration tests for restart policies and logging drivers

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    for ctr in "${ALL_CONTAINERS[@]}"; do
        local var="${ctr//-/_}_INSPECT"
        export "$var"="$(docker inspect "$ctr" 2>/dev/null || echo '[]')"
    done
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
}

_inspect() { local var="${1//-/_}_INSPECT"; echo "${!var}"; }

# Source: docker-compose.yml — all services have restart: unless-stopped

# ── Restart policy (7) ────────────────────────────────────────────────────

@test "resolver: restart policy is unless-stopped" {
    require_container "$CTR_RESOLVER"
    run jq -r '.[0].HostConfig.RestartPolicy.Name' <<< "$(_inspect "$CTR_RESOLVER")"
    assert_output "unless-stopped"
}

@test "gate: restart policy is unless-stopped" {
    require_container "$CTR_GATE"
    run jq -r '.[0].HostConfig.RestartPolicy.Name' <<< "$(_inspect "$CTR_GATE")"
    assert_output "unless-stopped"
}

@test "sentinel: restart policy is unless-stopped" {
    require_container "$CTR_SENTINEL"
    run jq -r '.[0].HostConfig.RestartPolicy.Name' <<< "$(_inspect "$CTR_SENTINEL")"
    assert_output "unless-stopped"
}

@test "scanner: restart policy is unless-stopped" {
    require_container "$CTR_SCANNER"
    run jq -r '.[0].HostConfig.RestartPolicy.Name' <<< "$(_inspect "$CTR_SCANNER")"
    assert_output "unless-stopped"
}

@test "state: restart policy is unless-stopped" {
    require_container "$CTR_STATE"
    run jq -r '.[0].HostConfig.RestartPolicy.Name' <<< "$(_inspect "$CTR_STATE")"
    assert_output "unless-stopped"
}

@test "toolbox: restart policy is unless-stopped" {
    require_container "$CTR_TOOLBOX"
    run jq -r '.[0].HostConfig.RestartPolicy.Name' <<< "$(_inspect "$CTR_TOOLBOX")"
    assert_output "unless-stopped"
}

@test "workspace: restart policy is unless-stopped" {
    require_container "$CTR_WORKSPACE"
    run jq -r '.[0].HostConfig.RestartPolicy.Name' <<< "$(_inspect "$CTR_WORKSPACE")"
    assert_output "unless-stopped"
}

# ── Logging driver (7) ────────────────────────────────────────────────────

@test "resolver: uses json-file logging driver" {
    require_container "$CTR_RESOLVER"
    run jq -r '.[0].HostConfig.LogConfig.Type' <<< "$(_inspect "$CTR_RESOLVER")"
    assert_output "json-file"
}

@test "gate: uses json-file logging driver" {
    require_container "$CTR_GATE"
    run jq -r '.[0].HostConfig.LogConfig.Type' <<< "$(_inspect "$CTR_GATE")"
    assert_output "json-file"
}

@test "sentinel: uses json-file logging driver" {
    require_container "$CTR_SENTINEL"
    run jq -r '.[0].HostConfig.LogConfig.Type' <<< "$(_inspect "$CTR_SENTINEL")"
    assert_output "json-file"
}

@test "scanner: uses json-file logging driver" {
    require_container "$CTR_SCANNER"
    run jq -r '.[0].HostConfig.LogConfig.Type' <<< "$(_inspect "$CTR_SCANNER")"
    assert_output "json-file"
}

@test "state: uses json-file logging driver" {
    require_container "$CTR_STATE"
    run jq -r '.[0].HostConfig.LogConfig.Type' <<< "$(_inspect "$CTR_STATE")"
    assert_output "json-file"
}

@test "toolbox: uses json-file logging driver" {
    require_container "$CTR_TOOLBOX"
    run jq -r '.[0].HostConfig.LogConfig.Type' <<< "$(_inspect "$CTR_TOOLBOX")"
    assert_output "json-file"
}

@test "workspace: uses json-file logging driver" {
    require_container "$CTR_WORKSPACE"
    run jq -r '.[0].HostConfig.LogConfig.Type' <<< "$(_inspect "$CTR_WORKSPACE")"
    assert_output "json-file"
}
