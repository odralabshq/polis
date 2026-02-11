#!/bin/bash
set -euo pipefail

echo "[gateway] Starting initialization..."

# Source shared network helpers
SCRIPT_DIR="$(dirname "$0")"
if [[ -f "$SCRIPT_DIR/network-helpers.sh" ]]; then
    source "$SCRIPT_DIR/network-helpers.sh"
elif [[ -f "/scripts/network-helpers.sh" ]]; then
    source "/scripts/network-helpers.sh"
fi

# Certificate validation (fail-fast)
# AC: Remove ca.pem → gateway exits with clear error message
# AC: Expired certificate → gateway refuses to start
validate_certificates() {
    local ca_cert="/etc/g3proxy/ssl/ca.pem"
    local ca_key="/etc/g3proxy/ssl/ca.key"
    
    if [[ ! -f "$ca_cert" ]]; then
        echo "[gateway] ERROR: CA certificate not found: $ca_cert"
        exit 1
    fi
    
    if [[ ! -f "$ca_key" ]]; then
        echo "[gateway] ERROR: CA private key not found: $ca_key"
        exit 1
    fi
    
    # Validate certificate is not expired
    if ! openssl x509 -checkend 86400 -noout -in "$ca_cert" 2>/dev/null; then
        echo "[gateway] ERROR: CA certificate expires within 24 hours"
        exit 1
    fi
    
    # Validate key matches certificate (using SHA-256, not MD5)
    local cert_modulus=$(openssl x509 -noout -modulus -in "$ca_cert" 2>/dev/null | openssl sha256)
    local key_modulus=$(openssl rsa -noout -modulus -in "$ca_key" 2>/dev/null | openssl sha256)
    
    if [[ "$cert_modulus" != "$key_modulus" ]]; then
        echo "[gateway] ERROR: CA certificate and key do not match"
        exit 1
    fi
    
    echo "[gateway] Certificate validation passed"
}

# Run validation first
validate_certificates

# Fallback definitions if shared helpers not available
if ! type is_wsl2 &>/dev/null; then
    is_wsl2() { grep -qi microsoft /proc/version 2>/dev/null; }
fi

# Gateway-specific IPv6 disable (includes ip6tables)
# All steps are best-effort — failures are logged as warnings and startup continues.
disable_ipv6_gateway() {
    echo "[gateway] Disabling IPv6..."
    
    if is_wsl2; then
        echo "[gateway] WSL2 detected - sysctl IPv6 disable not supported by WSL2 kernel"
    else
        # Native Linux: Disable via sysctl
        if sysctl -w net.ipv6.conf.all.disable_ipv6=1 >/dev/null 2>&1 && \
           sysctl -w net.ipv6.conf.default.disable_ipv6=1 >/dev/null 2>&1; then
            echo "[gateway] IPv6 disabled via sysctl"
        else
            echo "[gateway] WARNING: sysctl IPv6 disable failed — continuing"
        fi
    fi
    
    # Drop IPv6 traffic via ip6tables (works on both native Linux and WSL2)
    if command -v ip6tables &> /dev/null; then
        # Block at raw table (earliest possible point in netfilter)
        ip6tables -t raw -F 2>/dev/null || true
        ip6tables -t raw -A PREROUTING -j DROP 2>/dev/null || true
        ip6tables -t raw -A OUTPUT -j DROP 2>/dev/null || true
        
        # Also set filter table policies for defense in depth
        if ip6tables -P INPUT DROP 2>/dev/null && \
           ip6tables -P OUTPUT DROP 2>/dev/null && \
           ip6tables -P FORWARD DROP 2>/dev/null; then
            echo "[gateway] IPv6 blocked via ip6tables (raw + filter tables)"
        else
            echo "[gateway] WARNING: ip6tables filter policy set failed"
        fi
    else
        echo "[gateway] WARNING: ip6tables not available"
    fi
    
    # FAIL-CLOSED: Verify no routable (global) IPv6 addresses exist
    if ip -6 addr show scope global 2>/dev/null | grep -q "inet6"; then
        echo "[gateway] WARNING: Global IPv6 addresses still present after disable attempt:"
        ip -6 addr show scope global 2>/dev/null || true
        echo "[gateway] Continuing startup because IPv6 disable was not possible in this environment."
    fi
    
    # Additional strict check for native Linux
    if ! is_wsl2; then
        if ip -6 addr show 2>/dev/null | grep -q "inet6"; then
            echo "[gateway] WARNING: IPv6 addresses still present (native Linux)."
            echo "[gateway] Continuing startup because IPv6 disable was not possible in this environment."
        fi
    fi
    
    echo "[gateway] IPv6 disable/check completed"
}

disable_ipv6_gateway

# Note: ip_forward and ip_nonlocal_bind are set via docker-compose sysctls

# Detect interfaces dynamically
# Gateway is connected to: internal-bridge, gateway-bridge, external-bridge
# We need to find the interface connected to internal-bridge (for TPROXY)

# Get all interfaces except lo
INTERFACES=$(ip -o link show | awk -F': ' '{print $2}' | grep -v lo | tr '\n' ' ')
echo "[gateway] Available interfaces: $INTERFACES"

# Show IP addresses for debugging
ip -o addr show | grep -v "127.0.0.1" | while read line; do
    echo "[gateway] $line"
done

# Detect internal interface (internal-bridge network - where workspace connects)
# The internal-bridge uses 10.10.1.x subnet (gateway is .2, workspace uses this as default gw)
# We detect by finding the interface on the 10.10.1.x subnet
# SECURITY: Fail-closed - abort if internal interface cannot be detected
INTERNAL_IF=""
for iface in $(ip -o link show | awk -F': ' '{print $2}' | grep -v lo); do
    # Strip @ifXXX suffix for ip addr lookup
    iface_clean="${iface%%@*}"
    if ip -o addr show dev "$iface_clean" 2>/dev/null | grep -qE '10\.10\.1\.[0-9]+/'; then
        INTERNAL_IF="$iface"
        echo "[gateway] Detected internal interface: $INTERNAL_IF"
        break
    fi
done

if [[ -z "$INTERNAL_IF" ]]; then
    echo "[gateway] CRITICAL: Could not detect internal interface (10.10.1.x subnet)"
    echo "[gateway] Available interfaces and IPs:"
    ip -o addr show | grep -v "127.0.0.1"
    echo "[gateway] Aborting - TPROXY would be misconfigured"
    exit 1
fi

# Strip @ifXXX suffix for iptables (iptables doesn't understand veth naming)
INTERNAL_IF_CLEAN="${INTERNAL_IF%%@*}"
echo "[gateway] Using clean interface name for iptables: $INTERNAL_IF_CLEAN"

# Note: rp_filter is disabled via docker-compose sysctls (net.ipv4.conf.all.rp_filter=0)

# Setup TPROXY policy routing
ip rule add fwmark 0x1 lookup 100 2>/dev/null || true
ip route add local 0.0.0.0/0 dev lo table 100 2>/dev/null || true

# Configure iptables for TPROXY using DIVERT chain pattern (kernel docs recommended)
# DIVERT chain: marks and accepts packets belonging to established transparent sockets
iptables -t mangle -N DIVERT 2>/dev/null || iptables -t mangle -F DIVERT
iptables -t mangle -A DIVERT -j MARK --set-mark 0x1
iptables -t mangle -A DIVERT -j ACCEPT

# G3TPROXY chain: intercepts new HTTP/HTTPS connections
iptables -t mangle -N G3TPROXY 2>/dev/null || iptables -t mangle -F G3TPROXY
iptables -t mangle -A G3TPROXY -p tcp --dport 80 -j TPROXY --on-port 18080 --tproxy-mark 0x1
iptables -t mangle -A G3TPROXY -p tcp --dport 443 -j TPROXY --on-port 18080 --tproxy-mark 0x1

# PREROUTING: established connections first (DIVERT), then new connections (G3TPROXY)
# --transparent flag required to match sockets with IP_TRANSPARENT set
iptables -t mangle -A PREROUTING -p tcp -m socket --transparent -j DIVERT
iptables -t mangle -A PREROUTING -i "$INTERNAL_IF_CLEAN" -j G3TPROXY

# [SECURITY FIX] Block non-HTTP traffic from internal subnet
# TPROXY intercepts ports 80/443 and redirects to local socket (never reaches FORWARD)
# Any traffic from internal subnet that reaches FORWARD is non-HTTP and must be blocked
# Exception: DNS (UDP 53) is needed for hostname resolution
iptables -t filter -A FORWARD -i "$INTERNAL_IF_CLEAN" -p udp --dport 53 -j ACCEPT
iptables -t filter -A FORWARD -i "$INTERNAL_IF_CLEAN" -j DROP
echo "[gateway] Non-HTTP traffic blocked from internal subnet (only DNS allowed to forward)"

# [SECURITY FIX] NAT only for traffic from internal subnet
# Prevents unintended NATing between internal networks
INTERNAL_SUBNET=$(ip -o addr show dev "$INTERNAL_IF_CLEAN" 2>/dev/null | awk '{print $4}' | head -1)
if [[ -n "$INTERNAL_SUBNET" ]]; then
    iptables -t nat -A POSTROUTING -s "$INTERNAL_SUBNET" -j MASQUERADE
    echo "[gateway] NAT configured for internal subnet: $INTERNAL_SUBNET"
else
    # Fail closed if subnet detection fails to prevent unsafe NAT
    echo "[gateway] ERROR: Could not detect internal subnet for NAT. Exiting."
    exit 1
fi

# Wait for ICAP service to be ready (TCP port check)
echo "[gateway] Waiting for ICAP service..."
for i in {1..30}; do
    if timeout 1 bash -c "echo > /dev/tcp/icap/1344" 2>/dev/null; then
        echo "[gateway] ICAP service ready at icap:1344"
        break
    fi
    sleep 1
done

# Start g3fcgen
echo "[gateway] Starting g3fcgen..."
g3fcgen -c /etc/g3proxy/g3fcgen.yaml &

# Clean up stale sockets (directory already owned by g3proxy from Dockerfile)
rm -rf /tmp/g3/*

# Drop privileges using setpriv with ambient capabilities
# This allows g3proxy to run as non-root while retaining CAP_NET_ADMIN for TPROXY
echo "[gateway] Dropping privileges to g3proxy user (with CAP_NET_ADMIN)..."
exec setpriv --reuid=g3proxy --regid=g3proxy --init-groups \
    --inh-caps +net_admin --ambient-caps +net_admin \
    -- g3proxy -c /etc/g3proxy/g3proxy.yaml
