#!/usr/bin/env bats
# DNS Integration Tests — CoreDNS blocklist filtering (needs dns container)

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$DNS_CONTAINER"
}

# Helper: query CoreDNS from inside the dns container
dns_query() {
    docker exec "$DNS_CONTAINER" nslookup "$1" 127.0.0.1 2>&1
}

# =============================================================================
# Container Health
# =============================================================================

@test "dns: container is running" {
    assert_container_running "$DNS_CONTAINER"
}

@test "dns: container is healthy" {
    assert_container_healthy "$DNS_CONTAINER"
}

@test "dns: has static IP 10.30.1.10" {
    run docker inspect --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$DNS_CONTAINER"
    assert_success
    assert_output --partial "10.30.1.10"
}

# =============================================================================
# Blocked Domains → NXDOMAIN
# =============================================================================

@test "dns: blocks webhook.site (exfiltration)" {
    run dns_query "webhook.site"
    assert_output --partial "NXDOMAIN"
}

@test "dns: blocks ngrok.io (tunneling)" {
    run dns_query "ngrok.io"
    assert_output --partial "NXDOMAIN"
}

@test "dns: blocks ngrok-free.app (tunneling)" {
    run dns_query "ngrok-free.app"
    assert_output --partial "NXDOMAIN"
}

@test "dns: blocks transfer.sh (file sharing)" {
    run dns_query "transfer.sh"
    assert_output --partial "NXDOMAIN"
}

@test "dns: blocks burpcollaborator.net (OOB)" {
    run dns_query "burpcollaborator.net"
    assert_output --partial "NXDOMAIN"
}

@test "dns: blocks githab.com (typosquatting)" {
    run dns_query "githab.com"
    assert_output --partial "NXDOMAIN"
}

# =============================================================================
# Allowed Domains → Resolve
# =============================================================================

@test "dns: resolves github.com" {
    run dns_query "github.com"
    assert_success
    refute_output --partial "NXDOMAIN"
    assert_output --partial "Address"
}

@test "dns: resolves api.github.com" {
    run dns_query "api.github.com"
    assert_success
    refute_output --partial "NXDOMAIN"
}

@test "dns: resolves google.com" {
    run dns_query "google.com"
    assert_success
    refute_output --partial "NXDOMAIN"
}

# =============================================================================
# Config Mounted Correctly
# =============================================================================

@test "dns: Corefile mounted at /etc/coredns/Corefile" {
    assert_file_exists_in_container "$DNS_CONTAINER" "/etc/coredns/Corefile"
}

@test "dns: blocklist mounted at /etc/coredns/blocklist.txt" {
    assert_file_exists_in_container "$DNS_CONTAINER" "/etc/coredns/blocklist.txt"
}

@test "dns: no-new-privileges security opt" {
    run docker inspect --format '{{.HostConfig.SecurityOpt}}' "$DNS_CONTAINER"
    assert_success
    assert_output --partial "no-new-privileges"
}
