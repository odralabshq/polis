#!/usr/bin/env bats
# bats file_tags=e2e,toolbox
# MCP tool operations via Streamable HTTP transport (JSON-RPC)

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_TOOLBOX" "$CTR_STATE"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
}

teardown() {
    # request_id must be 12 chars; tests use req-e2eNNNNN format
    local rid
    rid=$(printf "req-e2e%05d" "$BATS_TEST_NUMBER")
    _admin_del "polis:blocked:${rid}" 2>/dev/null || true
    _admin_del "polis:approved:${rid}" 2>/dev/null || true
}

# -- helpers ------------------------------------------------------------------

_admin_del() {
    local key="$1"
    docker exec "$CTR_STATE" sh -c "
        REDISCLI_AUTH=\$(cat /run/secrets/valkey_mcp_admin_password) \
        valkey-cli --tls --cert /etc/valkey/tls/client.crt \
            --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
            --user mcp-admin --no-auth-warning DEL $key" 2>/dev/null || true
}

_agent_cli() {
    docker exec "$CTR_STATE" sh -c "
        REDISCLI_AUTH=\$(cat /run/secrets/valkey_mcp_agent_password) \
        valkey-cli --tls --cert /etc/valkey/tls/client.crt \
            --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
            --user mcp-agent --no-auth-warning $*"
}

# Send an MCP JSON-RPC tool call; returns only the JSON data line from SSE.
# Uses host curl since minimal toolbox image lacks curl.
mcp_call() {
    local tool="$1" args="$2"
    local toolbox_ip
    toolbox_ip=$(docker inspect -f '{{(index .NetworkSettings.Networks "polis_internal-bridge").IPAddress}}' "$CTR_TOOLBOX")
    local ep="https://${toolbox_ip}:8080/mcp"
    local ct="Content-Type:application/json"
    local ac="Accept:application/json,text/event-stream"
    local ca_cert="${PROJECT_ROOT}/certs/ca/ca.pem"

    # 1. Initialize session
    local init='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"0.1.0"}}}'
    local headers
    headers=$(mktemp)
    curl -sf --cacert "$ca_cert" -D "$headers" -o /dev/null -H "$ct" -H "$ac" --connect-timeout 10 \
        -X POST "$ep" -d "$init"

    local sid
    sid=$(grep -i mcp-session-id "$headers" | sed 's/.*: *//;s/\r//')
    rm -f "$headers"

    # 2. Initialized notification
    curl -sf --cacert "$ca_cert" -o /dev/null -H "$ct" -H "$ac" -H "Mcp-Session-Id:$sid" --connect-timeout 10 \
        -X POST "$ep" \
        -d '{"jsonrpc":"2.0","method":"notifications/initialized"}' 2>/dev/null || true

    # 3. Tool call — extract JSON from SSE data: lines
    local payload="{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"${tool}\",\"arguments\":${args}}}"
    curl -sf --cacert "$ca_cert" -H "$ct" -H "$ac" -H "Mcp-Session-Id:$sid" --connect-timeout 10 \
        -X POST "$ep" -d "$payload" 2>/dev/null \
        | grep '^data: {' | sed 's/^data: //'
}

# =============================================================================
# report_block
# =============================================================================

@test "e2e: report_block stores blocked request in Valkey" {
    local rid="req-e2e00001"
    _admin_del "polis:blocked:${rid}"

    run mcp_call "report_block" \
        "{\"request_id\":\"${rid}\",\"reason\":\"credential_detected\",\"destination\":\"https://evil.com\",\"pattern\":\"aws_secret\"}"
    assert_success
    assert_output --partial "${rid}"

    run _agent_cli GET "polis:blocked:${rid}"
    assert_success
    assert_output --partial "credential_detected"
}

@test "e2e: report_block sets TTL on blocked key" {
    local rid="req-e2e00002"
    _admin_del "polis:blocked:${rid}"

    run mcp_call "report_block" \
        "{\"request_id\":\"${rid}\",\"reason\":\"url_blocked\",\"destination\":\"https://x.com\"}"
    assert_success

    run _agent_cli TTL "polis:blocked:${rid}"
    assert_success
    [[ "$output" -gt 0 ]]
    [[ "$output" -le 3600 ]]
}

@test "e2e: report_block returns approval command" {
    local rid="req-e2e00003"
    _admin_del "polis:blocked:${rid}"

    run mcp_call "report_block" \
        "{\"request_id\":\"${rid}\",\"reason\":\"url_blocked\",\"destination\":\"https://blocked.com\"}"
    assert_success
    assert_output --partial "/polis-approve ${rid}"
}

@test "e2e: report_block redacts pattern from response" {
    local rid="req-e2e00004"
    _admin_del "polis:blocked:${rid}"

    run mcp_call "report_block" \
        "{\"request_id\":\"${rid}\",\"reason\":\"credential_detected\",\"destination\":\"https://evil.com\",\"pattern\":\"aws_secret_key\"}"
    assert_success
    refute_output --partial "aws_secret_key"

    # Pattern IS stored in Valkey
    run _agent_cli GET "polis:blocked:${rid}"
    assert_success
    assert_output --partial "aws_secret_key"
}

# =============================================================================
# check_request_status
# =============================================================================

@test "e2e: check_request_status returns pending" {
    local rid="req-e2e00005"
    _admin_del "polis:blocked:${rid}"

    run mcp_call "report_block" \
        "{\"request_id\":\"${rid}\",\"reason\":\"url_blocked\",\"destination\":\"https://x.com\"}"
    assert_success

    run mcp_call "check_request_status" "{\"request_id\":\"${rid}\"}"
    assert_success
    assert_output --partial "pending"
}

@test "e2e: check_request_status returns not_found for unknown ID" {
    local rid="req-e2effff6"
    _admin_del "polis:blocked:${rid}"
    _admin_del "polis:approved:${rid}"

    run mcp_call "check_request_status" "{\"request_id\":\"${rid}\"}"
    assert_success
    assert_output --partial "not_found"
}

# =============================================================================
# get_security_status / list_pending_approvals
# =============================================================================

@test "e2e: get_security_status returns valid JSON" {
    run mcp_call "get_security_status" "{}"
    assert_success
    assert_output --partial "pending_approvals"
    assert_output --partial "security_level"
}

@test "e2e: list_pending_approvals returns stored requests" {
    local rid="req-e2e00008"
    _admin_del "polis:blocked:${rid}"

    run mcp_call "report_block" \
        "{\"request_id\":\"${rid}\",\"reason\":\"url_blocked\",\"destination\":\"https://x.com\",\"pattern\":\"secret_pat\"}"
    assert_success

    run mcp_call "list_pending_approvals" "{}"
    assert_success
    assert_output --partial "${rid}"
}
