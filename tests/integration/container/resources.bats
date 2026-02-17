#!/usr/bin/env bats
# bats file_tags=integration,container
# Integration tests for container resource limits — memory, CPU, ulimits

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    for ctr in "$CTR_SENTINEL" "$CTR_SCANNER" "$CTR_STATE" "$CTR_WORKSPACE" "$CTR_GATE"; do
        local var="${ctr//-/_}_INSPECT"
        export "$var"="$(docker inspect "$ctr" 2>/dev/null || echo '[]')"
    done
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
}

# Source: docker-compose.yml resource limits

# ── Sentinel: mem_limit 3G, mem_reservation 1G ────────────────────────────

@test "sentinel: memory limit 3GB" {
    require_container "$CTR_SENTINEL"
    run jq -r '.[0].HostConfig.Memory' <<< "$polis_sentinel_INSPECT"
    assert_output "3221225472"
}

@test "sentinel: memory reservation 1GB" {
    require_container "$CTR_SENTINEL"
    run jq -r '.[0].HostConfig.MemoryReservation' <<< "$polis_sentinel_INSPECT"
    assert_output "1073741824"
}

# ── Scanner: deploy.resources 3G/1G ───────────────────────────────────────

@test "scanner: memory limit 3GB" {
    require_container "$CTR_SCANNER"
    run jq -r '.[0].HostConfig.Memory' <<< "$polis_scanner_INSPECT"
    assert_output "3221225472"
}

@test "scanner: memory reservation 1GB" {
    require_container "$CTR_SCANNER"
    run jq -r '.[0].HostConfig.MemoryReservation' <<< "$polis_scanner_INSPECT"
    assert_output "1073741824"
}

# ── State: deploy.resources 512M/256M, cpus 1.0 ──────────────────────────

@test "state: memory limit 512MB" {
    require_container "$CTR_STATE"
    run jq -r '.[0].HostConfig.Memory' <<< "$polis_state_INSPECT"
    assert_output "536870912"
}

@test "state: memory reservation 256MB" {
    require_container "$CTR_STATE"
    run jq -r '.[0].HostConfig.MemoryReservation' <<< "$polis_state_INSPECT"
    assert_output "268435456"
}

@test "state: CPU limit 1.0" {
    require_container "$CTR_STATE"
    run jq -r '.[0].HostConfig.NanoCpus' <<< "$polis_state_INSPECT"
    assert_output "1000000000"
}

# ── Workspace: deploy.resources 4G, cpus 2.0 ─────────────────────────────

@test "workspace: memory limit 4GB" {
    require_container "$CTR_WORKSPACE"
    run jq -r '.[0].HostConfig.Memory' <<< "$polis_workspace_INSPECT"
    assert_output "4294967296"
}

@test "workspace: CPU limit 2.0" {
    require_container "$CTR_WORKSPACE"
    run jq -r '.[0].HostConfig.NanoCpus' <<< "$polis_workspace_INSPECT"
    assert_output "2000000000"
}

# ── Gate: ulimits nofile 65536 ────────────────────────────────────────────

@test "gate: ulimits nofile 65536" {
    require_container "$CTR_GATE"
    run jq -r '.[0].HostConfig.Ulimits[] | select(.Name=="nofile") | .Soft' <<< "$polis_gate_INSPECT"
    assert_output "65536"
}
