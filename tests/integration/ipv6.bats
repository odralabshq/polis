#!/usr/bin/env bats
# Global IPv6 Security Tests

setup() {
    load "../helpers/common.bash"
    require_container "$GATEWAY_CONTAINER" "$WORKSPACE_CONTAINER"
}

@test "ipv6: workspace has no global IPv6 addresses" {
    run docker exec "${WORKSPACE_CONTAINER}" bash -c "ip -6 addr show scope global 2>/dev/null | grep -q inet6 && echo 'found' || echo 'none'"
    assert_success
    assert_output "none"
}

@test "ipv6: gateway has no global IPv6 addresses" {
    run docker exec "${GATEWAY_CONTAINER}" bash -c "ip -6 addr show scope global 2>/dev/null | grep -q inet6 && echo 'found' || echo 'none'"
    assert_success
    assert_output "none"
}

@test "ipv6: gateway nftables filter DROP policy" {
    # Verify that 'inet' table 'polis' has IPv6 drop rules in input and forward chains
    assert_nft_rule "${GATEWAY_CONTAINER}" "inet" "polis" "input" "nfproto ipv6 drop"
    assert_nft_rule "${GATEWAY_CONTAINER}" "inet" "polis" "forward" "nfproto ipv6 drop"
}
