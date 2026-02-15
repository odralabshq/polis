#!/usr/bin/env bash
# Network topology and connectivity assertions.

assert_on_network() {
    local ctr="$1" network="$2"
    local networks
    networks=$(docker inspect --format '{{json .NetworkSettings.Networks}}' "$ctr" 2>/dev/null)
    echo "$networks" | grep -q "\"$network\"" || fail "Expected $ctr on network $network"
}

assert_not_on_network() {
    local ctr="$1" network="$2"
    local networks
    networks=$(docker inspect --format '{{json .NetworkSettings.Networks}}' "$ctr" 2>/dev/null)
    echo "$networks" | grep -q "\"$network\"" && fail "Expected $ctr NOT on network $network" || true
}

assert_can_reach() {
    local from_ctr="$1" target_host="$2" target_port="$3"
    docker exec "$from_ctr" timeout 5 sh -c "echo > /dev/tcp/$target_host/$target_port" 2>/dev/null \
        || fail "Expected $from_ctr to reach $target_host:$target_port"
}

assert_cannot_reach() {
    local from_ctr="$1" target_host="$2" target_port="$3"
    docker exec "$from_ctr" timeout 5 sh -c "echo > /dev/tcp/$target_host/$target_port" 2>/dev/null \
        && fail "Expected $from_ctr NOT to reach $target_host:$target_port" || true
}

assert_port_listening() {
    local ctr="$1" port="$2"
    docker exec "$ctr" ss -tln 2>/dev/null | grep -q ":${port} " \
        || fail "Expected port $port listening in $ctr"
}
