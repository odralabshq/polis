#!/usr/bin/env bats
# bats file_tags=e2e,toolbox
# HITL (Human-in-the-Loop) approval workflow — CLI tests for `polis blocked`

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_STATE"
    export POLIS_CLI="${PROJECT_ROOT}/tools/blocked.sh"

    # Warm up toolbox connectivity from workspace (CI containers may need a moment)
    if docker exec "$CTR_WORKSPACE" test -f /tmp/agents/openclaw/scripts/polis-toolbox-call.sh 2>/dev/null; then
        for _i in 1 2 3; do
            docker exec "$CTR_WORKSPACE" \
                bash /tmp/agents/openclaw/scripts/polis-security-status.sh >/dev/null 2>&1 && break
            sleep 3
        done
    fi
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

# =============================================================================
# Workspace-side shell tool tests
# Verify polis-*.sh scripts work from inside the workspace container
# =============================================================================

@test "e2e: polis-security-status returns JSON from workspace" {
    require_agents_mounted
    run docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-security-status.sh
    assert_success
    assert_output --partial '"status"'
}

@test "e2e: polis-list-pending returns JSON from workspace" {
    require_agents_mounted
    run docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-list-pending.sh
    assert_success
    assert_output --partial '"pending"'
}

@test "e2e: polis-report-block from workspace stores request in Valkey" {
    require_agents_mounted
    local rid="req-e2e00a01"
    _admin_cmd DEL "polis:blocked:${rid}" 2>/dev/null || true

    run docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-report-block.sh \
        "$rid" "url_blocked" "https://ws-test.example.com"
    assert_success
    assert_output --partial '"requires_approval"'

    run _admin_cmd EXISTS "polis:blocked:${rid}"
    assert_output "1"

    _admin_cmd DEL "polis:blocked:${rid}" 2>/dev/null || true
}

@test "e2e: polis-check-status from workspace returns pending" {
    require_agents_mounted
    local rid="req-e2e00a02"
    _seed_blocked "$rid" "https://ws-check.example.com" "url_blocked"

    run docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-check-status.sh "$rid"
    assert_success
    assert_output --partial '"pending"'

    _admin_cmd DEL "polis:blocked:${rid}" 2>/dev/null || true
}

@test "e2e: polis-check-status from workspace returns approved after CLI approval" {
    require_agents_mounted
    local rid="req-e2e00a03"

    # Report via workspace shell script
    docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-report-block.sh \
        "$rid" "url_blocked" "https://ws-approve.example.com" 2>/dev/null || true

    # Approve via operator CLI
    bash "${PROJECT_ROOT}/tools/blocked.sh" approve "$rid"

    # Check from workspace
    run docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-check-status.sh "$rid"
    assert_success
    assert_output --partial '"approved"'

    _admin_cmd DEL "polis:approved:${rid}" 2>/dev/null || true
}

@test "e2e: polis-check-status from workspace returns not_found for unknown" {
    require_agents_mounted
    run docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-check-status.sh "req-e2e0dead"
    assert_success
    assert_output --partial '"not_found"'
}
