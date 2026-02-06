#!/bin/bash
# Shared network security helpers for Polis containers
# Used by: g3proxy-init.sh, workspace-init.sh

# Detect WSL2 environment (sysctl IPv6 disable doesn't work on WSL2)
is_wsl2() {
    grep -qi microsoft /proc/version 2>/dev/null
}

# Disable IPv6 with fail-closed verification
# Args: $1 = container name for logging (e.g., "gateway", "workspace")
disable_ipv6() {
    local container="${1:-container}"
    echo "[$container] Disabling IPv6..."
    
    if is_wsl2; then
        echo "[$container] WSL2 detected - sysctl IPv6 disable not supported by WSL2 kernel"
    else
        # Native Linux: Disable via sysctl
        if sysctl -w net.ipv6.conf.all.disable_ipv6=1 >/dev/null 2>&1 && \
           sysctl -w net.ipv6.conf.default.disable_ipv6=1 >/dev/null 2>&1; then
            echo "[$container] IPv6 disabled via sysctl"
        else
            echo "[$container] WARNING: sysctl IPv6 disable failed"
        fi
    fi
    
    # FAIL-CLOSED: Verify no routable (global) IPv6 addresses exist
    if ip -6 addr show scope global 2>/dev/null | grep -q "inet6"; then
        echo "[$container] CRITICAL: Global IPv6 addresses still present after disable attempt:"
        ip -6 addr show scope global 2>/dev/null || true
        echo "[$container] Aborting - TPROXY bypass risk"
        return 1
    fi
    
    # Additional strict check for native Linux (no IPv6 at all)
    if ! is_wsl2; then
        if ip -6 addr show 2>/dev/null | grep -q "inet6"; then
            echo "[$container] CRITICAL: IPv6 addresses still present (native Linux):"
            ip -6 addr show 2>/dev/null || true
            return 1
        fi
    fi
    
    echo "[$container] IPv6 verified disabled (no routable addresses)"
    return 0
}
