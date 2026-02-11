#!/usr/bin/env bats
# MCP-Agent E2E Tests
# Tests for MCP tool functionality through the Streamable HTTP transport.
# Verifies report_block, check_request_status, get_security_status,
# and list_pending_approvals via JSON-RPC over HTTP.
#
# Requirements: 6.6-6.8

setup() {
    TESTS_DIR="$(cd "${BATS_TEST_DIRNAME}/.." && pwd)"
    PROJECT_ROOT="$(cd "${TESTS_DIR}/.." && pwd)"
    load "${TESTS_DIR}/bats/bats-support/load.bash"
    load "${TESTS_DIR}/bats/bats-assert/load.bash"

    MCP_AGENT_CONTAINER="polis-mcp-agent"
    VALKEY_CONTAINER="polis-v2-valkey"
    MCP_ENDPOINT="http://localhost:8080/mcp"

    CREDENTIALS_FILE="${PROJECT_ROOT}/secrets/credentials.env.example"
}

# Helper: run a valkey-cli command inside the Valkey container
# as the mcp-agent ACL user.
# Usage: valkey_cli <command> [args...]
valkey_cli() {
    local agent_pass
    agent_pass="$(grep '^VALKEY_MCP_AGENT_PASS=' \
        "${CREDENTIALS_FILE}" | cut -d'=' -f2)"

    docker exec "${VALKEY_CONTAINER}" \
        valkey-cli \
        --tls \
        --cert /etc/valkey/tls/client.crt \
        --key /etc/valkey/tls/client.key \
        --cacert /etc/valkey/tls/ca.crt \
        --user mcp-agent \
        --pass "${agent_pass}" \
        "$@"
}

# Helper: send an MCP JSON-RPC tool call from inside the MCP-Agent
# container and capture the response.
# The Streamable HTTP transport requires an initialization handshake,
# so we call from within the container to localhost.
# Usage: mcp_call <tool_name> <arguments_json>
mcp_call() {
    local tool_name="$1"
    local arguments="$2"

    local init_payload='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"0.1.0"}}}'

    # Step 1: Initialize session and capture Mcp-Session-Id header
    local init_response
    init_response="$(docker exec "${MCP_AGENT_CONTAINER}" \
        curl -sf -D /tmp/mcp_headers -o /tmp/mcp_init_body \
        -X POST "${MCP_ENDPOINT}" \
        -H 'Content-Type: application/json' \
        -H 'Accept: application/json, text/event-stream' \
        --connect-timeout 10 \
        -d "${init_payload}" \
        && docker exec "${MCP_AGENT_CONTAINER}" \
        cat /tmp/mcp_headers 2>/dev/null)"

    local session_id
    session_id="$(echo "${init_response}" | \
        grep -i 'mcp-session-id' | \
        sed 's/.*: *//;s/\r//')"

    # Step 2: Send initialized notification
    docker exec "${MCP_AGENT_CONTAINER}" \
        curl -sf -o /dev/null \
        -X POST "${MCP_ENDPOINT}" \
        -H 'Content-Type: application/json' \
        -H 'Accept: application/json, text/event-stream' \
        -H "Mcp-Session-Id: ${session_id}" \
        --connect-timeout 10 \
        -d '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
        2>/dev/null || true

    # Step 3: Call the tool
    local tool_payload="{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"${tool_name}\",\"arguments\":${arguments}}}"

    docker exec "${MCP_AGENT_CONTAINER}" \
        curl -sf \
        -X POST "${MCP_ENDPOINT}" \
        -H 'Content-Type: application/json' \
        -H 'Accept: application/json, text/event-stream' \
        -H "Mcp-Session-Id: ${session_id}" \
        --connect-timeout 10 \
        -d "${tool_payload}" 2>/dev/null
}

# Helper: clean up a test key from Valkey (best-effort).
# The mcp-agent ACL user cannot DEL, so we use mcp-admin.
# Usage: cleanup_valkey_key <key>
cleanup_valkey_key() {
    local key="$1"
    local admin_pass
    admin_pass="$(grep '^VALKEY_MCP_ADMIN_PASS=' \
        "${CREDENTIALS_FILE}" | cut -d'=' -f2)"

    docker exec "${VALKEY_CONTAINER}" \
        valkey-cli \
        --tls \
        --cert /etc/valkey/tls/client.crt \
        --key /etc/valkey/tls/client.key \
        --cacert /etc/valkey/tls/ca.crt \
        --user mcp-admin \
        --pass "${admin_pass}" \
        DEL "${key}" 2>/dev/null || true
}

# =============================================================================
# report_block Tests (Requirement 6.7)
# =============================================================================

@test "e2e-mcp: report_block stores blocked request in Valkey" {
    local req_id="req-e2e00001"

    # Clean up any leftover key from a previous run
    cleanup_valkey_key "polis:blocked:${req_id}"

    # Call report_block via MCP
    run mcp_call "report_block" \
        "{\"request_id\":\"${req_id}\",\"reason\":\"credential_detected\",\"destination\":\"https://evil.com\",\"pattern\":\"aws_secret\"}"
    assert_success

    # Verify the response contains the request_id
    assert_output --partial "${req_id}"

    # Verify the Valkey key was created
    run valkey_cli GET "polis:blocked:${req_id}"
    assert_success
    assert_output --partial "${req_id}"
    assert_output --partial "credential_detected"
    assert_output --partial "https://evil.com"

    # Cleanup
    cleanup_valkey_key "polis:blocked:${req_id}"
}

@test "e2e-mcp: report_block sets TTL on blocked key" {
    local req_id="req-e2e00002"

    cleanup_valkey_key "polis:blocked:${req_id}"

    # Call report_block
    run mcp_call "report_block" \
        "{\"request_id\":\"${req_id}\",\"reason\":\"malware_domain\",\"destination\":\"https://malware.com\"}"
    assert_success

    # Verify TTL is set (should be between 1 and 3600 seconds)
    run valkey_cli TTL "polis:blocked:${req_id}"
    assert_success
    local ttl_val="${output}"
    # TTL must be positive (key exists with expiry) and <= 3600
    [[ "${ttl_val}" -gt 0 ]] || \
        fail "TTL should be > 0, got: ${ttl_val}"
    [[ "${ttl_val}" -le 3600 ]] || \
        fail "TTL should be <= 3600, got: ${ttl_val}"

    cleanup_valkey_key "polis:blocked:${req_id}"
}

@test "e2e-mcp: report_block returns approval command" {
    local req_id="req-e2e00003"

    cleanup_valkey_key "polis:blocked:${req_id}"

    run mcp_call "report_block" \
        "{\"request_id\":\"${req_id}\",\"reason\":\"url_blocked\",\"destination\":\"https://blocked.com\"}"
    assert_success

    # Response should contain the approval command
    assert_output --partial "polis approve ${req_id}"

    cleanup_valkey_key "polis:blocked:${req_id}"
}

@test "e2e-mcp: report_block redacts pattern from response" {
    local req_id="req-e2e00004"

    cleanup_valkey_key "polis:blocked:${req_id}"

    run mcp_call "report_block" \
        "{\"request_id\":\"${req_id}\",\"reason\":\"credential_detected\",\"destination\":\"https://evil.com\",\"pattern\":\"aws_secret_key\"}"
    assert_success

    # The pattern should NOT appear in the agent-facing response
    refute_output --partial "aws_secret_key"

    # But the pattern SHOULD be stored in Valkey
    run valkey_cli GET "polis:blocked:${req_id}"
    assert_success
    assert_output --partial "aws_secret_key"

    cleanup_valkey_key "polis:blocked:${req_id}"
}

# =============================================================================
# check_request_status Tests (Requirement 6.8)
# =============================================================================

@test "e2e-mcp: check_request_status returns pending for stored request" {
    local req_id="req-e2e00005"

    cleanup_valkey_key "polis:blocked:${req_id}"

    # First, store a blocked request
    run mcp_call "report_block" \
        "{\"request_id\":\"${req_id}\",\"reason\":\"credential_detected\",\"destination\":\"https://evil.com\"}"
    assert_success

    # Now check its status — should be "pending"
    run mcp_call "check_request_status" \
        "{\"request_id\":\"${req_id}\"}"
    assert_success
    assert_output --partial "pending"

    cleanup_valkey_key "polis:blocked:${req_id}"
}

@test "e2e-mcp: check_request_status returns not_found for unknown request" {
    # Use a request_id that was never stored
    local req_id="req-e2efffff"

    # Ensure the key doesn't exist
    cleanup_valkey_key "polis:blocked:${req_id}"
    cleanup_valkey_key "polis:approved:${req_id}"

    run mcp_call "check_request_status" \
        "{\"request_id\":\"${req_id}\"}"
    assert_success
    assert_output --partial "not_found"
}

# =============================================================================
# get_security_status Tests (Requirement 6.7)
# =============================================================================

@test "e2e-mcp: get_security_status returns valid JSON" {
    run mcp_call "get_security_status" "{}"
    assert_success

    # Response should contain expected fields
    assert_output --partial "pending_approvals"
    assert_output --partial "recent_approvals"
    assert_output --partial "security_level"
}

# =============================================================================
# list_pending_approvals Tests (Requirement 6.7)
# =============================================================================

@test "e2e-mcp: list_pending_approvals returns stored requests" {
    local req_id="req-e2e00006"

    cleanup_valkey_key "polis:blocked:${req_id}"

    # Store a blocked request first
    run mcp_call "report_block" \
        "{\"request_id\":\"${req_id}\",\"reason\":\"credential_detected\",\"destination\":\"https://evil.com\",\"pattern\":\"secret_pattern\"}"
    assert_success

    # List pending approvals — should include our request
    run mcp_call "list_pending_approvals" "{}"
    assert_success
    assert_output --partial "${req_id}"

    # Pattern should be redacted (null) in the response
    refute_output --partial "secret_pattern"

    cleanup_valkey_key "polis:blocked:${req_id}"
}

# =============================================================================
# Input Validation Tests (Requirement 6.7)
# =============================================================================

@test "e2e-mcp: report_block rejects invalid request_id" {
    # Invalid format — should be rejected before touching Valkey
    run mcp_call "report_block" \
        "{\"request_id\":\"bad-id\",\"reason\":\"credential_detected\",\"destination\":\"https://evil.com\"}"
    assert_success
    # The MCP response should contain an error about invalid request_id
    assert_output --partial "Invalid request_id"
}

@test "e2e-mcp: check_request_status rejects invalid request_id" {
    run mcp_call "check_request_status" \
        "{\"request_id\":\"not-valid!!\"}"
    assert_success
    assert_output --partial "Invalid request_id"
}
