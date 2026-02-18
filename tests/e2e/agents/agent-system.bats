#!/usr/bin/env bats
# bats file_tags=e2e,agents
# Agent system — workspace container, image tags, compose config

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    approve_host "example.com" 600
}

teardown_file() {
    true
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
}

# =============================================================================
# Workspace container
# =============================================================================

@test "e2e: workspace container exists" {
    run docker ps -a --format '{{.Names}}' --filter "name=^${CTR_WORKSPACE}$"
    assert_success
    assert_output "$CTR_WORKSPACE"
}

@test "e2e: workspace is running" {
    run docker ps --format '{{.Names}}' --filter "name=^${CTR_WORKSPACE}$"
    assert_success
    assert_output "$CTR_WORKSPACE"
}

@test "e2e: active agent determined from image tag" {
    run docker inspect --format '{{.Config.Image}}' "$CTR_WORKSPACE"
    assert_success
    assert_output --regexp "polis-workspace(-oss)?:(base|openclaw|latest)"
}

# =============================================================================
# docker-compose contract
# =============================================================================

@test "e2e: docker-compose workspace uses latest tag" {
    # Source: docker-compose.yml → image: polis-workspace-oss:latest
    run grep "polis-workspace-oss:latest" "$PROJECT_ROOT/docker-compose.yml"
    assert_success
}

@test "e2e: docker-compose healthcheck includes ip route" {
    # Source: healthcheck test: systemctl is-active polis-init.service && ip route | grep -q default
    run grep "ip route" "$PROJECT_ROOT/docker-compose.yml"
    assert_success
}

# =============================================================================
# Network access
# =============================================================================

@test "e2e: workspace can access HTTP via TPROXY" {
    run_with_network_skip "example.com" \
        docker exec "$CTR_WORKSPACE" \
        curl -sf -o /dev/null -w "%{http_code}" --connect-timeout 15 \
        http://example.com
    assert_success
    assert_output "200"
}
