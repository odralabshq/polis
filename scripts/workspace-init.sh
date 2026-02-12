#!/bin/bash
set -euo pipefail

echo "[workspace] Starting initialization..."

# Update CA certificates
update-ca-certificates 2>/dev/null || true

# Source shared network helpers
SCRIPT_DIR="$(dirname "$0")"
if [[ -f "$SCRIPT_DIR/network-helpers.sh" ]]; then
    source "$SCRIPT_DIR/network-helpers.sh"
elif [[ -f "/usr/local/bin/network-helpers.sh" ]]; then
    source "/usr/local/bin/network-helpers.sh"
fi

# Fallback definitions if shared helpers not available
if ! type is_wsl2 &>/dev/null; then
    is_wsl2() { grep -qi microsoft /proc/version 2>/dev/null; }
fi

if ! type disable_ipv6 &>/dev/null; then
    # Disable IPv6 at kernel level (Sysbox virtualizes procfs, so this works without --privileged)
    # SECURITY: Fail-closed - abort if IPv6 cannot be verified disabled
    disable_ipv6() {
        local container="${1:-workspace}"
        echo "[$container] Disabling IPv6..."
        
        if is_wsl2; then
            echo "[$container] WSL2 detected - sysctl IPv6 disable not supported by WSL2 kernel"
            echo "[$container] Relying on Docker network-level disable (enable_ipv6: false)"
        else
            # Native Linux: Disable via sysctl - Sysbox virtualizes /proc/sys per container
            if sysctl -w net.ipv6.conf.all.disable_ipv6=1 >/dev/null 2>&1 && \
               sysctl -w net.ipv6.conf.default.disable_ipv6=1 >/dev/null 2>&1; then
                echo "[$container] IPv6 disabled via sysctl"
            else
                echo "[$container] WARNING: sysctl IPv6 disable failed"
            fi
        fi
        
        # FAIL-CLOSED: Verify no routable (global) IPv6 addresses exist
        # Note: Link-local (fe80::) may persist on WSL2 but is not routable/bypassable
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
fi

# Protect sensitive paths — defense-in-depth layer (secondary to tmpfs mounts)
# chmod 000 existing dirs, create decoys for missing ones
protect_sensitive_paths() {
    local paths=(".ssh" ".aws" ".gnupg" ".config/gcloud" ".kube" ".docker")
    local home_dir="${HOME:-/root}"

    echo "[workspace] Protecting sensitive paths..."
    for p in "${paths[@]}"; do
        local full_path="$home_dir/$p"
        if [[ -d "$full_path" ]]; then
            chmod 000 "$full_path"
            echo "[workspace] Protected existing: $full_path"
        else
            mkdir -p "$full_path"
            chmod 000 "$full_path"
            echo "[workspace] Created decoy: $full_path"
        fi
    done
    echo "[workspace] Sensitive paths protected (6 paths)"
}

disable_ipv6 "workspace" || exit 1

# Detect gateway IP dynamically via DNS
GATEWAY_IP=$(getent hosts gateway | awk '{print $1}')

if [[ -z "$GATEWAY_IP" ]]; then
    # Fallback: detect gateway from routing table
    GATEWAY_IP=$(ip route | grep default | awk '{print $3}')
fi

if [[ -z "$GATEWAY_IP" ]]; then
    echo "[workspace] ERROR: Could not determine gateway IP"
    exit 1
fi

echo "[workspace] Gateway IP: $GATEWAY_IP"

# Configure routing
ip route del default 2>/dev/null || true
ip route add default via $GATEWAY_IP

# Protect sensitive directories (defense-in-depth, secondary to tmpfs mounts)
protect_sensitive_paths

# Bootstrap mounted agents (fault-isolated from core init)
for agent_dir in /tmp/agents/*/; do
    [ -d "$agent_dir" ] || continue
    name=$(basename "$agent_dir")
    echo "[workspace] Bootstrapping agent: ${name}"

    # Run install.sh in a subshell so failures don't kill workspace init
    if [ -x "${agent_dir}/install.sh" ]; then
        if ! ("${agent_dir}/install.sh"); then
            echo "[workspace] WARNING: ${name}/install.sh failed — agent may not work"
            continue
        fi
    fi

    # Enable + start the agent service (if service file was mounted)
    svc="/etc/systemd/system/${name}.service"
    if [ -f "$svc" ]; then
        systemctl daemon-reload
        systemctl enable --now "${name}.service" || \
            echo "[workspace] WARNING: failed to enable ${name}.service"
    fi
done

echo "[workspace] Initialization complete"
