#!/bin/bash
set -euo pipefail

echo "[workspace] Starting initialization..."

# Update CA certificates — ensure Polis CA is in the system bundle.
# The CA cert is bind-mounted read-only with non-root ownership, which
# prevents update-ca-certificates from auto-detecting it on some Debian
# versions. We copy it to /usr/share/ca-certificates/ (writable) first.
if [[ -f /usr/local/share/ca-certificates/polis-ca.crt ]]; then
    cp /usr/local/share/ca-certificates/polis-ca.crt \
       /usr/share/ca-certificates/polis-ca.crt 2>/dev/null || true
    grep -qxF 'polis-ca.crt' /etc/ca-certificates.conf 2>/dev/null || \
        echo 'polis-ca.crt' >> /etc/ca-certificates.conf
fi
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

# Configure default route to gate for TPROXY FIRST — the workspace is on an
# internal-only Docker network with no default gateway. Without this route,
# there is zero internet connectivity (install.sh, apt-get, curl all fail).
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

# Bootstrap mounted agents AFTER routing is configured so they have internet.
# Traffic goes through the gate's transparent proxy (TPROXY), which handles
# HTTP/HTTPS fine for package downloads.
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

    # Service enablement is handled below (with integrity checks)

    # Symlink polis-* scripts into PATH so the agent can invoke them
    # directly without searching the filesystem (find / returns exit 1
    # due to permission-denied directories, confusing the agent).
    for script in "${agent_dir}"/scripts/polis-*.sh; do
        [ -f "$script" ] || continue
        base=$(basename "$script" .sh)
        ln -sf "$script" "/usr/local/bin/${base}"
    done
done

# Protect sensitive directories (defense-in-depth, secondary to tmpfs mounts)
protect_sensitive_paths

# Collect and start agent services (with integrity verification from manifest system).
# install.sh already ran above (before routing), so we only handle .service files here.
agent_services=()
for agent_dir in /tmp/agents/*/; do
    [ -d "$agent_dir" ] || continue
    name=$(basename "$agent_dir")

    # Collect services to enable (generated .service file is mounted by compose override)
    svc="/etc/systemd/system/${name}.service"
    if [ -f "$svc" ]; then
        # Verify .service file integrity (hash generated at polis init time)
        hash_file="/etc/systemd/system/${name}.service.sha256"
        if [ -f "$hash_file" ]; then
            expected=$(cat "$hash_file")
            actual=$(sha256sum "$svc" | cut -d' ' -f1)
            if [ "$expected" != "$actual" ]; then
                echo "[workspace] CRITICAL: ${name}.service integrity check failed. Skipping."
                continue
            fi
            echo "[workspace] ${name}.service integrity verified"
        fi
        agent_services+=("${name}.service")
    fi
done

# Single daemon-reload, then enable and start all collected services.
# IMPORTANT: Use --no-block to avoid deadlock. Agent services declare
# Requires=polis-init.service, so they wait for this script to finish.
# Using "enable --now" (which blocks until the service is active) would
# deadlock: init waits for agent → agent waits for init.
if [ ${#agent_services[@]} -gt 0 ]; then
    systemctl daemon-reload
    for svc in "${agent_services[@]}"; do
        systemctl enable "$svc" || \
            echo "[workspace] WARNING: failed to enable ${svc}"
        systemctl start --no-block "$svc" || \
            echo "[workspace] WARNING: failed to queue start for ${svc}"
    done
fi

echo "[workspace] Initialization complete"
exit 0
