#!/bin/bash
# Shared network security helpers for Polis containers
# Used by: g3proxy-init.sh, workspace-init.sh

# Disable IPv6 with fail-closed verification
# Args: $1 = container name for logging (e.g., "gateway", "workspace")
disable_ipv6() {
    local container="${1:-container}"
    echo "[$container] Disabling IPv6..."
    
    # Native Linux + Sysbox: Disable via sysctl
    if sysctl -w net.ipv6.conf.all.disable_ipv6=1 >/dev/null 2>&1 && \
       sysctl -w net.ipv6.conf.default.disable_ipv6=1 >/dev/null 2>&1; then
        echo "[$container] IPv6 disabled via sysctl"
    else
        echo "[$container] WARNING: sysctl IPv6 disable failed"
    fi
    
    # FAIL-CLOSED: Verify no IPv6 addresses exist
    if ip -6 addr show 2>/dev/null | grep -q "inet6"; then
        echo "[$container] CRITICAL: IPv6 addresses still present after disable attempt:"
        ip -6 addr show 2>/dev/null || true
        echo "[$container] Aborting - TPROXY bypass risk"
        return 1
    fi
    
    echo "[$container] IPv6 verified disabled"
    return 0
}
