# Polis Network Test Helpers
# Extracted from gateway-ipv6.bats (reusable by ipv6.bats, hardening.bats)

ip6tables_functional() {
    docker exec "$1" ip6tables -L -n &>/dev/null
}

sysctl_ipv6_functional() {
    docker exec "$1" sysctl -n net.ipv6.conf.all.disable_ipv6 &>/dev/null
}

ipv6_disabled() {
    ! docker exec "$1" bash -c "ip -6 addr show scope global 2>/dev/null | grep -q inet6"
}
