#!/usr/bin/env bash
# Security hardening assertions.

assert_cap_drop_all() {
    local ctr="$1"
    local caps
    caps=$(docker inspect --format '{{json .HostConfig.CapDrop}}' "$ctr" 2>/dev/null)
    echo "$caps" | grep -qi "ALL" || fail "Expected $ctr cap_drop ALL, got: $caps"
}

assert_cap_add() {
    local ctr="$1" cap="$2"
    local caps
    caps=$(docker inspect --format '{{json .HostConfig.CapAdd}}' "$ctr" 2>/dev/null)
    echo "$caps" | grep -q "$cap" || fail "Expected $ctr cap_add $cap, got: $caps"
}

assert_no_new_privs() {
    local ctr="$1"
    local secopt
    secopt=$(docker inspect --format '{{json .HostConfig.SecurityOpt}}' "$ctr" 2>/dev/null)
    echo "$secopt" | grep -q "no-new-privileges" || fail "Expected $ctr no-new-privileges, got: $secopt"
}

assert_not_privileged() {
    local ctr="$1"
    local priv
    priv=$(docker inspect --format '{{.HostConfig.Privileged}}' "$ctr" 2>/dev/null)
    [[ "$priv" == "false" ]] || fail "Expected $ctr not privileged, got: $priv"
}

assert_read_only_rootfs() {
    local ctr="$1"
    local ro
    ro=$(docker inspect --format '{{.HostConfig.ReadonlyRootfs}}' "$ctr" 2>/dev/null)
    [[ "$ro" == "true" ]] || fail "Expected $ctr read-only rootfs, got: $ro"
}

assert_seccomp_applied() {
    local ctr="$1"
    local secopt
    secopt=$(docker inspect --format '{{json .HostConfig.SecurityOpt}}' "$ctr" 2>/dev/null)
    echo "$secopt" | grep -q "seccomp" || fail "Expected $ctr seccomp profile, got: $secopt"
}
