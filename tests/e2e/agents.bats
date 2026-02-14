#!/usr/bin/env bats
# bats file_tags=e2e,agents
# Agent System E2E Tests — Manifest-driven Plugin System

setup() {
    load '../helpers/common.bash'
    load '../bats/bats-file/load.bash'
}

# --- Agent manifest contract tests ---

@test "agents: openclaw agent.yaml exists" {
    assert_file_exist "${PROJECT_ROOT}/agents/openclaw/agent.yaml"
}

@test "agents: openclaw install.sh exists and is executable" {
    assert_file_exist "${PROJECT_ROOT}/agents/openclaw/install.sh"
    run test -x "${PROJECT_ROOT}/agents/openclaw/install.sh"
    assert_success
}

@test "agents: openclaw has required scripts" {
    assert_file_exist "${PROJECT_ROOT}/agents/openclaw/scripts/init.sh"
}

@test "agents: template directory exists" {
    assert_file_exist "${PROJECT_ROOT}/agents/_template/agent.yaml"
    assert_file_exist "${PROJECT_ROOT}/agents/_template/install.sh"
}

@test "agents: openclaw agent.yaml has required fields" {
    local manifest="${PROJECT_ROOT}/agents/openclaw/agent.yaml"
    run grep -q 'apiVersion:' "$manifest"
    assert_success
    run grep -q 'kind: AgentPlugin' "$manifest"
    assert_success
    run grep -q 'metadata:' "$manifest"
    assert_success
    run grep -q 'name: openclaw' "$manifest"
    assert_success
    run grep -q 'spec:' "$manifest"
    assert_success
}

@test "agents: openclaw manifest has runtime command" {
    run grep -q 'command:' "${PROJECT_ROOT}/agents/openclaw/agent.yaml"
    assert_success
}

@test "agents: openclaw manifest has health check" {
    run grep -q 'health:' "${PROJECT_ROOT}/agents/openclaw/agent.yaml"
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
