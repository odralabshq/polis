#!/usr/bin/env bats
# bats file_tags=integration,network,security
# Integration tests for cross-network isolation

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    load "../../lib/assertions/network.bash"
}

# ── Workspace isolation ───────────────────────────────────────────────────

@test "workspace: cannot reach sentinel directly" {
    require_container "$CTR_WORKSPACE"
    assert_cannot_reach "$CTR_WORKSPACE" "$IP_SENTINEL" "$PORT_ICAP"
}

@test "workspace: cannot reach external-bridge" {
    require_container "$CTR_WORKSPACE"
    assert_cannot_reach "$CTR_WORKSPACE" "$IP_GATE_EXT" 80
}

@test "workspace: default route via gate" {
    require_container "$CTR_WORKSPACE"
    run docker exec "$CTR_WORKSPACE" ip route show default
    assert_success
    assert_output --partial "$IP_GATE_INT"
}

@test "workspace: has at most 2 interfaces plus lo" {
    # Base: 1 interface (internal-bridge). Agent overrides (e.g. openclaw)
    # may add a host-access network → 2 interfaces. Never more than 2.
    require_container "$CTR_WORKSPACE"
    run docker exec "$CTR_WORKSPACE" sh -c "ip -o link show | grep -cv lo"
    assert_success
    [[ "$output" -ge 1 && "$output" -le 2 ]] || fail "Expected 1-2 interfaces, got $output"
}

@test "workspace: cannot reach cloud metadata" {
    require_container "$CTR_WORKSPACE"
    assert_cannot_reach "$CTR_WORKSPACE" "169.254.169.254" 80
}

@test "workspace: HTTP to gateway-bridge IP blocked" {
    require_container "$CTR_WORKSPACE"
    assert_cannot_reach "$CTR_WORKSPACE" "$IP_SENTINEL" 80
}

# ── Sentinel isolation ────────────────────────────────────────────────────

@test "sentinel: cannot reach workspace" {
    require_container "$CTR_SENTINEL" "$CTR_WORKSPACE"
    local ws_ip
    ws_ip=$(docker inspect "$CTR_WORKSPACE" 2>/dev/null | jq -r ".[0].NetworkSettings.Networks.\"$NET_INTERNAL\".IPAddress")
    [[ -n "$ws_ip" && "$ws_ip" != "null" ]] || skip "workspace IP not found on internal-bridge"
    assert_cannot_reach "$CTR_SENTINEL" "$ws_ip" 22
}

# ── Gate interface count ──────────────────────────────────────────────────

@test "gate: has 3 interfaces plus lo" {
    require_container "$CTR_GATE"
    run docker exec "$CTR_GATE" sh -c "ip -o link show | grep -cv lo"
    assert_success
    assert_output "3"
}

# ── Scanner isolation ─────────────────────────────────────────────────────

@test "scanner: cannot reach internal-bridge" {
    require_container "$CTR_SCANNER"
    assert_cannot_reach "$CTR_SCANNER" "$IP_GATE_INT" 80
}

@test "scanner: cannot reach workspace" {
    require_container "$CTR_SCANNER" "$CTR_WORKSPACE"
    local ws_ip
    ws_ip=$(docker inspect "$CTR_WORKSPACE" 2>/dev/null | jq -r ".[0].NetworkSettings.Networks.\"$NET_INTERNAL\".IPAddress")
    [[ -n "$ws_ip" && "$ws_ip" != "null" ]] || skip "workspace IP not found on internal-bridge"
    assert_cannot_reach "$CTR_SCANNER" "$ws_ip" 22
}
