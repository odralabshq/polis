#!/usr/bin/env bats
# bats file_tags=integration,toolbox
# MCP-Agent Container Unit Tests
# Tests for polis-mcp-agent container
# Requirements: 6.1-6.5

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$MCP_AGENT_CONTAINER"
}

# =============================================================================
# Container State Tests (Requirement 6.2)
# =============================================================================

@test "mcp-agent: container exists" {
    run docker ps -a --filter "name=${MCP_AGENT_CONTAINER}" --format '{{.Names}}'
    assert_success
    assert_output "${MCP_AGENT_CONTAINER}"
}

@test "mcp-agent: container is running" {
    run docker ps --filter "name=${MCP_AGENT_CONTAINER}" --format '{{.Status}}'
    assert_success
    assert_output --partial "Up"
}

@test "mcp-agent: container is healthy" {
    run docker inspect --format '{{.State.Health.Status}}' "${MCP_AGENT_CONTAINER}"
    assert_success
    assert_output "healthy"
}

# =============================================================================
# Network Tests (Requirement 6.3)
# =============================================================================

@test "mcp-agent: connected to internal-bridge network" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${MCP_AGENT_CONTAINER}"
    assert_success
    assert_output --partial "internal-bridge"
}

@test "mcp-agent: connected to gateway-bridge network" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${MCP_AGENT_CONTAINER}"
    assert_success
    assert_output --partial "gateway-bridge"
}

@test "mcp-agent: not connected to external-bridge network" {
    run docker inspect --format '{{range $net, $config := .NetworkSettings.Networks}}{{$net}} {{end}}' "${MCP_AGENT_CONTAINER}"
    assert_success
    refute_output --partial "external-bridge"
}

# =============================================================================
# Security Tests (Requirement 6.4)
# =============================================================================

@test "mcp-agent: has no-new-privileges security option" {
    run docker inspect --format '{{.HostConfig.SecurityOpt}}' "${MCP_AGENT_CONTAINER}"
    assert_success
    assert_output --partial "no-new-privileges"
}

@test "mcp-agent: drops all capabilities" {
    run docker inspect --format '{{.HostConfig.CapDrop}}' "${MCP_AGENT_CONTAINER}"
    assert_success
    assert_output --partial "ALL"
}

# =============================================================================
# Health Endpoint Tests (Requirement 6.5)
# =============================================================================

@test "mcp-agent: health endpoint responds on port 8080" {
    run docker exec "${MCP_AGENT_CONTAINER}" curl -sf http://localhost:8080/health
    assert_success
}

# =============================================================================
# Environment Tests (Requirement 6.1)
# =============================================================================

@test "mcp-agent: polis_AGENT_LISTEN_ADDR is set" {
    run docker exec "${MCP_AGENT_CONTAINER}" printenv polis_AGENT_LISTEN_ADDR
    assert_success
    assert_output "0.0.0.0:8080"
}

@test "mcp-agent: polis_AGENT_VALKEY_URL is set" {
    run docker exec "${MCP_AGENT_CONTAINER}" printenv polis_AGENT_VALKEY_URL
    assert_success
    assert_output "rediss://state:6379"
}

@test "mcp-agent: polis_AGENT_VALKEY_USER is set" {
    run docker exec "${MCP_AGENT_CONTAINER}" printenv polis_AGENT_VALKEY_USER
    assert_success
    assert_output "mcp-agent"
}

# =============================================================================
# Logging Tests (Requirement 4.9)
# =============================================================================

@test "mcp-agent: uses json-file logging driver" {
    run docker inspect --format '{{.HostConfig.LogConfig.Type}}' "${MCP_AGENT_CONTAINER}"
    assert_success
    assert_output "json-file"
}

# =============================================================================
# Restart Policy Tests (Requirement 4.10)
# =============================================================================

@test "mcp-agent: restart policy is unless-stopped" {
    run docker inspect --format '{{.HostConfig.RestartPolicy.Name}}' "${MCP_AGENT_CONTAINER}"
    assert_success
    assert_output "unless-stopped"
}