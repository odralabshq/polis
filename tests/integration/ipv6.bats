#!/usr/bin/env bats
# bats file_tags=integration,network
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

@test "ipv6: gateway ip6tables filter DROP policy" {
    if ! docker exec "${GATEWAY_CONTAINER}" ip6tables -L -n &>/dev/null; then
        skip "ip6tables not functional in this environment"
    fi
    run docker exec "${GATEWAY_CONTAINER}" ip6tables -L INPUT -n
    assert_success
    assert_output --partial "policy DROP"
}
