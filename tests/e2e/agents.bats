#!/usr/bin/env bats
# Agent System E2E Tests

setup() {
    load '../helpers/common.bash'
    load '../bats/bats-file/load.bash'
}

# --- Agent contract tests (depend on Issue 01 only) ---

@test "agents: openclaw agent.conf exists" {
    assert_file_exist "${PROJECT_ROOT}/agents/openclaw/agent.conf"
}

@test "agents: openclaw install.sh exists and is executable" {
    assert_file_exist "${PROJECT_ROOT}/agents/openclaw/install.sh"
    run test -x "${PROJECT_ROOT}/agents/openclaw/install.sh"
    assert_success
}

@test "agents: openclaw has required scripts" {
    assert_file_exist "${PROJECT_ROOT}/agents/openclaw/scripts/init.sh"
    assert_file_exist "${PROJECT_ROOT}/agents/openclaw/config/openclaw.service"
}

@test "agents: openclaw compose.override.yaml exists" {
    assert_file_exist "${PROJECT_ROOT}/agents/openclaw/compose.override.yaml"
}

@test "agents: template directory exists" {
    assert_file_exist "${PROJECT_ROOT}/agents/_template/agent.conf"
    assert_file_exist "${PROJECT_ROOT}/agents/_template/install.sh"
    assert_file_exist "${PROJECT_ROOT}/agents/_template/config/agent.service"
}

@test "agents: template service requires polis-init" {
    run grep -q 'Requires=polis-init.service' "${PROJECT_ROOT}/agents/_template/config/agent.service"
    assert_success
    run grep -q 'After=.*polis-init.service' "${PROJECT_ROOT}/agents/_template/config/agent.service"
    assert_success
}

@test "agents: openclaw agent.conf has required fields" {
    local conf="${PROJECT_ROOT}/agents/openclaw/agent.conf"
    run grep -q '^AGENT_NAME=' "$conf"
    assert_success
    run grep -q '^AGENT_SERVICE_NAME=' "$conf"
    assert_success
    run grep -q '^AGENT_CONTAINER_PORT=' "$conf"
    assert_success
}

@test "agents: docker-compose has no profiles directives" {
    run grep -q 'profiles:' "${PROJECT_ROOT}/docker-compose.yml"
    assert_failure
}

@test "agents: docker-compose workspace uses latest tag" {
    run grep -q 'polis-workspace-oss:latest' "${PROJECT_ROOT}/docker-compose.yml"
    assert_success
}

@test "agents: docker-compose base healthcheck includes ip route" {
    run grep 'systemctl is-active polis-init.service.*ip route' "${PROJECT_ROOT}/docker-compose.yml"
    assert_success
}

# --- Runtime tests (require running containers) ---

@test "agents: workspace container exists" {
    run docker ps -a --format '{{.Names}}' --filter "name=polis-workspace"
    assert_success
    assert_output "polis-workspace"
}

@test "agents: workspace is running" {
    run docker ps --format '{{.Names}}' --filter "name=polis-workspace"
    assert_success
    assert_output "polis-workspace"
}

@test "agents: can determine active agent from image tag" {
    run docker inspect --format '{{.Config.Image}}' polis-workspace
    assert_success
    assert_output --regexp "polis-workspace(-oss)?:(base|openclaw|latest)"
}
