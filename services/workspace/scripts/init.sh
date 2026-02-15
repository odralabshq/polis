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


if ! type disable_ipv6 &>/dev/null; then
    # Disable IPv6 at kernel level (Sysbox virtualizes procfs, so this works without --privileged)
    # SECURITY: Fail-closed - abort if IPv6 cannot be verified disabled
    disable_ipv6() {
        local container="${1:-workspace}"
        echo "[$container] Disabling IPv6..."
        
        # Native Linux + Sysbox: Disable via sysctl (procfs is virtualized per container)
        if sysctl -w net.ipv6.conf.all.disable_ipv6=1 >/dev/null 2>&1 && \
           sysctl -w net.ipv6.conf.default.disable_ipv6=1 >/dev/null 2>&1; then
            echo "[$container] IPv6 disabled via sysctl"
        else
            echo "[$container] WARNING: sysctl IPv6 disable failed"
        fi
        
        # FAIL-CLOSED: Verify no IPv6 addresses exist at all
        if ip -6 addr show 2>/dev/null | grep -q "inet6"; then
            echo "[$container] CRITICAL: IPv6 addresses still present after disable attempt:"
            ip -6 addr show 2>/dev/null || true
            echo "[$container] Aborting - TPROXY bypass risk"
            return 1
        fi
        
        echo "[$container] IPv6 verified disabled"
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

# Bootstrap mounted agents BEFORE routing through the proxy.
# Agent install scripts (install.sh) need raw internet access to fetch packages
# (apt-get, npm, git clone, etc.). The transparent proxy intercepts and may block
# plain HTTP traffic, causing install failures (403 Forbidden from TPROXY).
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

    # Enable the agent service (if service file was mounted).
    # NOTE: We use "enable" without "--now" because this init script runs as
    # polis-init.service (Type=oneshot). Agent services declare
    # Requires=polis-init.service + After=polis-init.service, so starting them
    # here would deadlock (systemd waits for polis-init to finish first).
    # Instead, we enable the service and start it explicitly after this loop.
    svc="/etc/systemd/system/${name}.service"
    if [ -f "$svc" ]; then
        systemctl daemon-reload
        systemctl enable "${name}.service" || \
            echo "[workspace] WARNING: failed to enable ${name}.service"
    fi
done

# Configure default route to gate for TPROXY
# Note: Docker doesn't configure gateways for internal networks, so we must do it manually
echo "[workspace] Resolving gate IP..."
GATE_IP=$(getent hosts gate | awk '{print $1}')

if [[ -z "$GATE_IP" ]]; then
    echo "[workspace] ERROR: Could not resolve 'gate' service"
    exit 1
fi

echo "[workspace] Configuring default route via gate (${GATE_IP})..."

# Remove any existing default route
ip route del default 2>/dev/null || true

# Add default route through gate
if ip route add default via $GATE_IP; then
    echo "[workspace] Default route configured successfully"
    ip route show
else
    echo "[workspace] ERROR: Failed to configure default route"
    exit 1
fi

# Protect sensitive directories (defense-in-depth, secondary to tmpfs mounts)
protect_sensitive_paths

# Start agent services that were enabled above.
# polis-init.service is about to exit, so systemd will consider it "active (exited)"
# and the After=polis-init.service dependency will be satisfied.
for agent_dir in /tmp/agents/*/; do
    [ -d "$agent_dir" ] || continue
    name=$(basename "$agent_dir")
    svc="/etc/systemd/system/${name}.service"
    if [ -f "$svc" ] && systemctl is-enabled "${name}.service" &>/dev/null; then
        echo "[workspace] Queuing start for ${name}.service (will start after init completes)"
        # Use --no-block so we don't wait for the service to fully start
        systemctl start --no-block "${name}.service" || \
            echo "[workspace] WARNING: failed to queue ${name}.service start"
    fi
done

echo "[workspace] Initialization complete"
exit 0
