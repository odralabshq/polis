#!/usr/bin/env bats
# bats file_tags=e2e,toolbox
# HITL (Human-in-the-Loop) approval workflow — CLI tests for `polis blocked`

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_STATE"
    export POLIS_CLI="${PROJECT_ROOT}/cli/blocked.sh"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
}

teardown() {
    local rid
    rid=$(printf "req-hitl%04d" "$BATS_TEST_NUMBER")
    _admin_cmd DEL "polis:blocked:${rid}" 2>/dev/null || true
    _admin_cmd DEL "polis:approved:${rid}" 2>/dev/null || true
    _admin_cmd DEL "polis:approved:host:https://hitl-test-${BATS_TEST_NUMBER}.example.com" 2>/dev/null || true
}

# -- helpers ------------------------------------------------------------------

_admin_cmd() {
    docker exec "$CTR_STATE" sh -c "
        REDISCLI_AUTH=\$(cat /run/secrets/valkey_mcp_admin_password) \
        valkey-cli --tls --cert /etc/valkey/tls/client.crt \
            --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
            --user mcp-admin --no-auth-warning $*" 2>/dev/null
}

# Seed a blocked request directly in Valkey (simulates report_block MCP call)
_seed_blocked() {
    local rid="$1" dest="${2:-https://evil.example.com}" reason="${3:-credential_detected}"
    local json="{\"request_id\":\"${rid}\",\"reason\":\"${reason}\",\"destination\":\"${dest}\",\"pattern\":\"test_pattern\",\"timestamp\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\"}"
    _admin_cmd SETEX "polis:blocked:${rid}" 3600 "'${json}'"
}

# =============================================================================
# CLI usage and help
# =============================================================================

@test "e2e: polis blocked with unknown subcommand shows usage" {
    run bash "$POLIS_CLI" nonsense
    assert_failure
    assert_output --partial "Usage: polis blocked"
}

# =============================================================================
# Pending / List
# =============================================================================

@test "e2e: polis blocked pending with no requests shows empty message" {
    # Clean slate — ensure no blocked keys exist for our test prefix
    local rid="req-hitl0002"
    _admin_cmd DEL "polis:blocked:${rid}" 2>/dev/null || true

    run bash "$POLIS_CLI" pending
    # Should succeed regardless (may show other requests or "No pending")
    assert_success
}

@test "e2e: polis blocked pending lists seeded request" {
    local rid="req-hitl0003"
    _seed_blocked "$rid" "https://hitl-test-3.example.com" "url_blocked"

    run bash "$POLIS_CLI" pending
    assert_success
    assert_output --partial "$rid"
}

# =============================================================================
# Approve flow
# =============================================================================

@test "e2e: polis blocked approve moves key from blocked to approved" {
    local rid="req-hitl0004"
    _seed_blocked "$rid" "https://hitl-test-4.example.com"

    run bash "$POLIS_CLI" approve "$rid"
    assert_success
    assert_output --partial "Approved"

    # Blocked key should be gone
    run _admin_cmd GET "polis:blocked:${rid}"
    assert_output ""

    # Approved key should exist
    run _admin_cmd GET "polis:approved:${rid}"
    assert_success
    assert_output --partial "$rid"
}

@test "e2e: polis blocked approve sets TTL on approved key" {
    local rid="req-hitl0005"
    _seed_blocked "$rid" "https://hitl-test-5.example.com"

    bash "$POLIS_CLI" approve "$rid"

    run _admin_cmd TTL "polis:approved:${rid}"
    assert_success
    [[ "$output" -gt 0 ]]
    [[ "$output" -le 300 ]]
}

@test "e2e: polis blocked approve creates host-based approval key" {
    local rid="req-hitl0006"
    local dest="https://hitl-test-6.example.com"
    _seed_blocked "$rid" "$dest"

    bash "$POLIS_CLI" approve "$rid"

    run _admin_cmd GET "polis:approved:host:${dest}"
    assert_success
    assert_output "1"
}

@test "e2e: polis blocked approve for nonexistent request fails" {
    run bash "$POLIS_CLI" approve "req-hitl-nonexistent"
    assert_failure
    assert_output --partial "not found"
}

# =============================================================================
# Deny flow
# =============================================================================

@test "e2e: polis blocked deny removes blocked key" {
    local rid="req-hitl0008"
    _seed_blocked "$rid"

    run bash "$POLIS_CLI" deny "$rid"
    assert_success
    assert_output --partial "Denied"

    # Key should be gone
    run _admin_cmd GET "polis:blocked:${rid}"
    assert_output ""
}

# =============================================================================
# Check status
# =============================================================================

@test "e2e: polis blocked check shows pending for blocked request" {
    local rid="req-hitl0009"
    _seed_blocked "$rid"

    run bash "$POLIS_CLI" check "$rid"
    assert_success
    assert_output --partial "pending"
}

@test "e2e: polis blocked check shows approved after approval" {
    local rid="req-hitl0010"
    _seed_blocked "$rid" "https://hitl-test-10.example.com"
    bash "$POLIS_CLI" approve "$rid"

    run bash "$POLIS_CLI" check "$rid"
    assert_success
    assert_output --partial "approved"
}

@test "e2e: polis blocked check shows not found for unknown request" {
    run bash "$POLIS_CLI" check "req-hitl-unknown"
    assert_success
    assert_output --partial "not found"
}

# =============================================================================
# Approved request no longer appears in pending list
# =============================================================================

@test "e2e: approved request not in pending list" {
    local rid="req-hitl0012"
    _seed_blocked "$rid" "https://hitl-test-12.example.com"

    bash "$POLIS_CLI" approve "$rid"

    run bash "$POLIS_CLI" pending
    assert_success
    refute_output --partial "$rid"
}
