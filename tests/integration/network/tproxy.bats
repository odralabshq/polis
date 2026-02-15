#!/usr/bin/env bats
# bats file_tags=integration,network
# Integration tests for TPROXY and nftables rules on the gateway

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_GATE"
    # Cache nftables ruleset once
    export NFT_RULESET="$(docker exec "$CTR_GATE" nft list ruleset 2>/dev/null || echo '')"
    export NFT_TABLES="$(docker exec "$CTR_GATE" nft list tables 2>/dev/null || echo '')"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    load "../../lib/assertions/network.bash"
    require_container "$CTR_GATE"
}

# ── g3proxy listening ─────────────────────────────────────────────────────

@test "tproxy: g3proxy listening on port 18080" {
    assert_port_listening "$CTR_GATE" "$PORT_TPROXY"
}

# ── Policy routing ────────────────────────────────────────────────────────

@test "tproxy: ip rule for fwmark 0x2 exists" {
    run docker exec "$CTR_GATE" ip rule show
    assert_output --partial "fwmark 0x2"
}

@test "tproxy: routing table 102 has local route" {
    run docker exec "$CTR_GATE" ip route show table 102
    assert_success
    assert_output --partial "local"
}

# ── nftables table ────────────────────────────────────────────────────────

@test "tproxy: nft inet polis table exists" {
    run echo "$NFT_TABLES"
    assert_output --partial "inet polis"
}

@test "tproxy: no masquerade rule" {
    run echo "$NFT_RULESET"
    refute_output --partial "masquerade"
}

# ── nftables chains and rules ─────────────────────────────────────────────
# Source: services/gate/scripts/setup-network.sh

@test "tproxy: forward chain policy is drop" {
    run docker exec "$CTR_GATE" nft list chain inet polis forward
    assert_output --partial "policy drop"
}

@test "tproxy: TPROXY rule in prerouting_tproxy" {
    run docker exec "$CTR_GATE" nft list chain inet polis prerouting_tproxy
    assert_output --partial "tproxy to :18080"
}

@test "tproxy: DNS DNAT rule exists" {
    run echo "$NFT_RULESET"
    assert_output --partial "dnat ip to $IP_RESOLVER_GW"
}

@test "tproxy: IPv6 drop in prerouting" {
    run echo "$NFT_RULESET"
    assert_output --partial "meta nfproto ipv6 drop"
}

@test "tproxy: internal subnets excluded from TPROXY" {
    run docker exec "$CTR_GATE" nft list chain inet polis prerouting_tproxy
    assert_output --partial "$SUBNET_INTERNAL"
    assert_output --partial "$SUBNET_GATEWAY"
    assert_output --partial "$SUBNET_EXTERNAL"
}

# ── Docker DNS preserved ──────────────────────────────────────────────────

@test "tproxy: Docker DNS rules preserved" {
    run docker exec "$CTR_GATE" cat /etc/resolv.conf
    assert_output --partial "127.0.0.11"
}
