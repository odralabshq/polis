#!/usr/bin/env bats
# bats file_tags=integration,service
# Integration tests for the polis-toolbox HITL service.
# Verifies the MCP HTTP API is reachable and responds correctly.
# Requires: polis-toolbox and polis-state containers running.

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_STATE" "$CTR_TOOLBOX"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
}

# ── Health check ──────────────────────────────────────────────────────────────

@test "toolbox: health endpoint responds 200" {
    run docker exec "$CTR_TOOLBOX" \
        sh -c 'exec 3<>/dev/tcp/localhost/8080 && echo "OK"'
    assert_success
}

# ── MCP protocol via shell scripts ───────────────────────────────────────────
# These tests run the polis-*.sh scripts from inside the workspace container
# (which has the Polis CA trusted and can reach toolbox over HTTPS).

@test "toolbox: polis-security-status returns JSON from workspace" {
    require_container "$CTR_WORKSPACE"
    run docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-security-status.sh
    assert_success
    assert_output --partial '"status"'
}

@test "toolbox: polis-list-pending returns JSON from workspace" {
    require_container "$CTR_WORKSPACE"
    run docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-list-pending.sh
    assert_success
    assert_output --partial '"pending"'
}

@test "toolbox: polis-report-block stores request in Valkey" {
    require_container "$CTR_WORKSPACE"
    local rid="req-inttest01"

    # Clean up any leftover state
    _admin_cmd DEL "polis:blocked:${rid}" 2>/dev/null || true

    run docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-report-block.sh \
        "$rid" "url_blocked" "https://integration-test.example.com"
    assert_success
    assert_output --partial '"requires_approval"'

    # Verify the key landed in Valkey
    run _admin_cmd EXISTS "polis:blocked:${rid}"
    assert_success
    assert_output "1"

    # Cleanup
    _admin_cmd DEL "polis:blocked:${rid}" 2>/dev/null || true
}

@test "toolbox: polis-check-status returns pending for blocked request" {
    require_container "$CTR_WORKSPACE"
    local rid="req-inttest02"

    # Seed a blocked request via the shell script
    docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-report-block.sh \
        "$rid" "url_blocked" "https://check-test.example.com" 2>/dev/null || true

    run docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-check-status.sh "$rid"
    assert_success
    assert_output --partial '"pending"'

    # Cleanup
    _admin_cmd DEL "polis:blocked:${rid}" 2>/dev/null || true
}

@test "toolbox: polis-check-status returns not_found for unknown request" {
    require_container "$CTR_WORKSPACE"
    run docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-check-status.sh "req-doesnotexist"
    assert_success
    assert_output --partial '"not_found"'
}

@test "toolbox: polis-check-status returns approved after CLI approval" {
    require_container "$CTR_WORKSPACE"
    local rid="req-inttest03"

    # Seed via shell script
    docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-report-block.sh \
        "$rid" "url_blocked" "https://approve-test.example.com" 2>/dev/null || true

    # Approve via blocked.sh (the operator CLI)
    run bash "${PROJECT_ROOT}/cli/blocked.sh" approve "$rid"
    assert_success

    # Shell script should now see it as approved
    run docker exec "$CTR_WORKSPACE" \
        bash /tmp/agents/openclaw/scripts/polis-check-status.sh "$rid"
    assert_success
    assert_output --partial '"approved"'

    # Cleanup
    _admin_cmd DEL "polis:approved:${rid}" 2>/dev/null || true
}

# ── Helper ────────────────────────────────────────────────────────────────────

_admin_cmd() {
    local pass
    pass=$(cat "${PROJECT_ROOT}/secrets/valkey_mcp_admin_password.txt" 2>/dev/null) || return 1
    docker exec "$CTR_STATE" valkey-cli \
        --tls \
        --cert /etc/valkey/tls/client.crt \
        --key /etc/valkey/tls/client.key \
        --cacert /etc/valkey/tls/ca.crt \
        -a "$pass" --user mcp-admin --no-auth-warning \
        "$@"
}
