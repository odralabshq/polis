#!/usr/bin/env bats
# bats file_tags=integration,network,security
# Integration tests for IPv6 disabled enforcement

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
}

@test "workspace: no global IPv6 addresses" {
    require_container "$CTR_WORKSPACE"
    run docker exec "$CTR_WORKSPACE" ip -6 addr show scope global
    refute_output --partial "inet6"
}

@test "gate: no global IPv6 addresses" {
    require_container "$CTR_GATE"
    run docker exec "$CTR_GATE" ip -6 addr show scope global
    refute_output --partial "inet6"
}

@test "workspace: sysctl disable_ipv6=1" {
    require_container "$CTR_WORKSPACE"
    run docker exec "$CTR_WORKSPACE" sysctl -n net.ipv6.conf.all.disable_ipv6
    assert_success
    assert_output "1"
}

@test "gate: nftables drops IPv6" {
    require_container "$CTR_GATE"
    run docker exec "$CTR_GATE" nft list ruleset
    assert_output --partial "meta nfproto ipv6 drop"
}

@test "gate-init: logs show IPv6 disable" {
    run docker logs "$CTR_GATE_INIT" 2>&1
    assert_output --partial "Disabling IPv6"
}

@test "gate-init: logs show completion" {
    run docker logs "$CTR_GATE_INIT" 2>&1
    assert_output --partial "Networking setup complete"
}
