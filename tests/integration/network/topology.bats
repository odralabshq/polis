#!/usr/bin/env bats
# bats file_tags=integration,network
# Integration tests for network topology — container-to-network assignments

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    load "../../lib/assertions/network.bash"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    load "../../lib/assertions/network.bash"
}

# ── Resolver (3 networks) ─────────────────────────────────────────────────

@test "resolver: on gateway-bridge" {
    require_container "$CTR_RESOLVER"
    assert_on_network "$CTR_RESOLVER" "$NET_GATEWAY"
}

@test "resolver: on internal-bridge" {
    require_container "$CTR_RESOLVER"
    assert_on_network "$CTR_RESOLVER" "$NET_INTERNAL"
}

@test "resolver: on external-bridge" {
    require_container "$CTR_RESOLVER"
    assert_on_network "$CTR_RESOLVER" "$NET_EXTERNAL"
}

# ── Gate (3 networks) ─────────────────────────────────────────────────────

@test "gate: on internal-bridge" {
    require_container "$CTR_GATE"
    assert_on_network "$CTR_GATE" "$NET_INTERNAL"
}

@test "gate: on gateway-bridge" {
    require_container "$CTR_GATE"
    assert_on_network "$CTR_GATE" "$NET_GATEWAY"
}

@test "gate: on external-bridge" {
    require_container "$CTR_GATE"
    assert_on_network "$CTR_GATE" "$NET_EXTERNAL"
}

# ── Sentinel (gateway-bridge only) ────────────────────────────────────────

@test "sentinel: on gateway-bridge only" {
    require_container "$CTR_SENTINEL"
    assert_on_network "$CTR_SENTINEL" "$NET_GATEWAY"
    assert_not_on_network "$CTR_SENTINEL" "$NET_INTERNAL"
    assert_not_on_network "$CTR_SENTINEL" "$NET_EXTERNAL"
}

# ── Scanner ────────────────────────────────────────────────────────────────

@test "scanner: on gateway-bridge" {
    require_container "$CTR_SCANNER"
    assert_on_network "$CTR_SCANNER" "$NET_GATEWAY"
}

@test "scanner: on internet network" {
    require_container "$CTR_SCANNER"
    assert_on_network "$CTR_SCANNER" "$NET_INTERNET"
}

@test "scanner: NOT on internal-bridge" {
    require_container "$CTR_SCANNER"
    assert_not_on_network "$CTR_SCANNER" "$NET_INTERNAL"
}

# ── State (gateway-bridge only) ───────────────────────────────────────────

@test "state: on gateway-bridge only" {
    require_container "$CTR_STATE"
    assert_on_network "$CTR_STATE" "$NET_GATEWAY"
    assert_not_on_network "$CTR_STATE" "$NET_INTERNAL"
    assert_not_on_network "$CTR_STATE" "$NET_EXTERNAL"
}

# ── Toolbox ────────────────────────────────────────────────────────────────

@test "toolbox: on internal-bridge" {
    require_container "$CTR_TOOLBOX"
    assert_on_network "$CTR_TOOLBOX" "$NET_INTERNAL"
}

@test "toolbox: on gateway-bridge" {
    require_container "$CTR_TOOLBOX"
    assert_on_network "$CTR_TOOLBOX" "$NET_GATEWAY"
}

@test "toolbox: NOT on external-bridge" {
    require_container "$CTR_TOOLBOX"
    assert_not_on_network "$CTR_TOOLBOX" "$NET_EXTERNAL"
}

# ── Workspace (internal-bridge only) ──────────────────────────────────────

@test "workspace: on internal-bridge" {
    require_container "$CTR_WORKSPACE"
    assert_on_network "$CTR_WORKSPACE" "$NET_INTERNAL"
    assert_not_on_network "$CTR_WORKSPACE" "$NET_GATEWAY"
    assert_not_on_network "$CTR_WORKSPACE" "$NET_EXTERNAL"
}

@test "workspace: on default network when agent ports published" {
    require_container "$CTR_WORKSPACE"
    local ports
    ports=$(docker port "$CTR_WORKSPACE" 2>/dev/null || true)
    if [[ -n "$ports" ]]; then
        # Agent ports are published — workspace must also be on the default network
        # because internal-bridge (internal: true) cannot publish ports to the host
        assert_on_network "$CTR_WORKSPACE" "polis_default"
    else
        # No agent ports — workspace should only be on internal-bridge
        assert_not_on_network "$CTR_WORKSPACE" "polis_default"
    fi
}

# ── Network properties ────────────────────────────────────────────────────

@test "internal-bridge: is internal" {
    run docker network inspect "$NET_INTERNAL" --format '{{.Internal}}'
    assert_output "true"
}

@test "gateway-bridge: is internal" {
    run docker network inspect "$NET_GATEWAY" --format '{{.Internal}}'
    assert_output "true"
}

@test "all networks: IPv6 disabled" {
    for net in "$NET_INTERNAL" "$NET_GATEWAY" "$NET_EXTERNAL" "$NET_INTERNET"; do
        local v6
        v6=$(docker network inspect "$net" --format '{{.EnableIPv6}}' 2>/dev/null)
        [[ "$v6" == "false" ]] || fail "IPv6 enabled on $net"
    done
}

@test "no containers expose ports to host except workspace agent ports" {
    for ctr in "${ALL_CONTAINERS[@]}"; do
        local ports
        ports=$(docker port "$ctr" 2>/dev/null || true)
        if [[ "$ctr" == "$CTR_WORKSPACE" ]]; then
            # Workspace may expose agent ports (e.g. 18789 for openclaw dashboard).
            # If ports are published, verify none are reserved platform ports.
            if [[ -n "$ports" ]]; then
                for rp in $RESERVED_PORTS; do
                    if echo "$ports" | grep -qE "^${rp}/"; then
                        fail "$ctr exposes reserved platform port ${rp}: $ports"
                    fi
                done
            fi
        else
            [[ -z "$ports" ]] || fail "$ctr exposes ports: $ports"
        fi
    done
}

@test "internet network exists" {
    run docker network inspect "$NET_INTERNET" --format '{{.Name}}'
    assert_success
}
