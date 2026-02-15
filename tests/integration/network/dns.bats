#!/usr/bin/env bats
# bats file_tags=integration,network,dns
# Integration tests for DNS resolution and blocking via CoreDNS

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
}

# ── Resolver static IP ────────────────────────────────────────────────────

@test "resolver: has static IP on gateway-bridge" {
    require_container "$CTR_RESOLVER"
    run docker inspect "$CTR_RESOLVER"
    local ip
    ip=$(echo "$output" | jq -r ".[0].NetworkSettings.Networks.\"$NET_GATEWAY\".IPAddress")
    assert_equal "$ip" "$IP_RESOLVER_GW"
}

# ── Blocked domains (NXDOMAIN) ────────────────────────────────────────────
# Source: services/resolver/config/blocklist.txt

@test "dns: blocks webhook.site" {
    require_container "$CTR_RESOLVER"
    run docker exec "$CTR_RESOLVER" nslookup webhook.site 127.0.0.1
    assert_failure
}

@test "dns: blocks ngrok.io" {
    require_container "$CTR_RESOLVER"
    run docker exec "$CTR_RESOLVER" nslookup ngrok.io 127.0.0.1
    assert_failure
}

@test "dns: blocks ngrok-free.app" {
    require_container "$CTR_RESOLVER"
    run docker exec "$CTR_RESOLVER" nslookup ngrok-free.app 127.0.0.1
    assert_failure
}

@test "dns: blocks transfer.sh" {
    require_container "$CTR_RESOLVER"
    run docker exec "$CTR_RESOLVER" nslookup transfer.sh 127.0.0.1
    assert_failure
}

@test "dns: blocks burpcollaborator.net" {
    require_container "$CTR_RESOLVER"
    run docker exec "$CTR_RESOLVER" nslookup burpcollaborator.net 127.0.0.1
    assert_failure
}

@test "dns: blocks githab.com (typosquatting)" {
    require_container "$CTR_RESOLVER"
    run docker exec "$CTR_RESOLVER" nslookup githab.com 127.0.0.1
    assert_failure
}

# ── Allowed domains ───────────────────────────────────────────────────────

@test "dns: resolves github.com" {
    require_container "$CTR_RESOLVER"
    run docker exec "$CTR_RESOLVER" nslookup github.com 127.0.0.1
    assert_success
}

@test "dns: resolves google.com" {
    require_container "$CTR_RESOLVER"
    run docker exec "$CTR_RESOLVER" nslookup google.com 127.0.0.1
    assert_success
}

# ── Config files mounted ──────────────────────────────────────────────────

@test "resolver: Corefile mounted" {
    require_container "$CTR_RESOLVER"
    run docker exec "$CTR_RESOLVER" test -f /etc/coredns/Corefile
    assert_success
}

@test "resolver: blocklist mounted" {
    require_container "$CTR_RESOLVER"
    run docker exec "$CTR_RESOLVER" test -f /etc/coredns/blocklist.txt
    assert_success
}

# ── Security ──────────────────────────────────────────────────────────────

@test "resolver: has no-new-privileges" {
    require_container "$CTR_RESOLVER"
    run docker inspect --format '{{.HostConfig.SecurityOpt}}' "$CTR_RESOLVER"
    assert_output --partial "no-new-privileges"
}
