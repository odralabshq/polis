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
# ICAP modules — active pipeline
# =============================================================================

@test "e2e: DLP REQMOD module exists (credcheck)" {
    run docker exec "$CTR_SENTINEL" test -f /usr/lib/c_icap/srv_polis_dlp.so
    assert_success
}

@test "e2e: Sentinel RESPMOD module exists (ClamAV + OTT)" {
    run docker exec "$CTR_SENTINEL" test -f /usr/lib/c_icap/srv_polis_sentinel_resp.so
    assert_success
}

@test "e2e: credcheck REQMOD service responds" {
    run docker exec "$CTR_SENTINEL" \
        c-icap-client -i 127.0.0.1 -p 1344 -s credcheck -f /dev/null
    assert_success
}

@test "e2e: sentinel_respmod RESPMOD service responds" {
    run docker exec "$CTR_SENTINEL" \
        c-icap-client -i 127.0.0.1 -p 1344 -s sentinel_respmod -f /dev/null
    assert_success
}

@test "e2e: dead modules removed from image" {
    run docker exec "$CTR_SENTINEL" test -f /usr/lib/c_icap/srv_polis_approval_rewrite.so
    assert_failure
    run docker exec "$CTR_SENTINEL" test -f /usr/lib/c_icap/srv_polis_approval.so
    assert_failure
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
