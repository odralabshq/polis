#!/usr/bin/env bats
# bats file_tags=e2e,toolbox
# Approval system ICAP modules and ACL enforcement

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_SENTINEL" "$CTR_STATE"
}

# =============================================================================
# ICAP modules
# =============================================================================

@test "e2e: REQMOD approval rewriter module exists" {
    run docker exec "$CTR_SENTINEL" test -f /usr/lib/c_icap/srv_polis_approval_rewrite.so
    assert_success
}

@test "e2e: RESPMOD approval scanner module exists" {
    run docker exec "$CTR_SENTINEL" test -f /usr/lib/c_icap/srv_polis_approval.so
    assert_success
}

@test "e2e: REQMOD approval service responds" {
    # c-icap-client returns 204 (no modification needed) for empty request
    run docker exec "$CTR_SENTINEL" \
        c-icap-client -i 127.0.0.1 -p 1344 -s polis_approval_rewrite -f /dev/null
    assert_success
}

@test "e2e: RESPMOD approval service responds" {
    run docker exec "$CTR_SENTINEL" \
        c-icap-client -i 127.0.0.1 -p 1344 -s polis_approval
    assert_success
    assert_output --partial "200 OK"
}

# =============================================================================
# ACL enforcement
# =============================================================================

@test "e2e: mcp-agent cannot DEL keys (audit trail protection)" {
    # Source: mcp-agent ACL has -@all +GET +SET +SETEX ... (no +DEL)
    # Self-approval prevention is via time-gate (15s), not ACL.
    # ACL prevents the agent from deleting blocked/approved keys.
    run docker exec "$CTR_STATE" sh -c "
        REDISCLI_AUTH=\$(cat /run/secrets/valkey_mcp_agent_password) \
        valkey-cli --tls --cert /etc/valkey/tls/client.crt \
            --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
            --user mcp-agent --no-auth-warning \
            DEL polis:blocked:nonexistent-test-key"
    assert_output --partial "NOPERM"
}
