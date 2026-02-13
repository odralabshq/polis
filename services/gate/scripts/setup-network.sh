#!/bin/bash
set -euo pipefail

# =============================================================================
# Polis Gateway Networking Setup (Native Linux, Zero-Trust)
# =============================================================================

echo "[gate-init] Starting networking setup..."

# Service addresses (from docker-compose.yml static assignments)
RESOLVER_IP="10.30.1.10"
INTERNAL_SUBNET="10.10.1.0/24"
GATEWAY_SUBNET="10.30.1.0/24"
EXTERNAL_SUBNET="10.20.1.0/24"

# Detect internal interface (bridge connected to workspace)
INTERNAL_IF=$(ip -4 route show "$INTERNAL_SUBNET" | awk '{print $3}' | head -n 1)
if [ -z "$INTERNAL_IF" ]; then
    echo "[gate-init] ERROR: Could not detect internal interface for $INTERNAL_SUBNET"
    exit 1
fi
echo "[gate-init] Detected internal interface: $INTERNAL_IF"

# Get all local IPs for TPROXY exclusion
LOCAL_IPS=$(ip -4 addr show | grep inet | awk '{print $2}' | cut -d/ -f1 | tr '\n' ',' | sed 's/,$//')
LOCAL_IPS=${LOCAL_IPS:-127.0.0.1}

# Disable IPv6 (defense-in-depth, also enforced in nftables)
echo "[gate-init] Disabling IPv6..."
sysctl -w net.ipv6.conf.all.disable_ipv6=1 || true
sysctl -w net.ipv6.conf.default.disable_ipv6=1 || true

# Disable rp_filter (required for TPROXY)
echo "[gate-init] Disabling rp_filter..."
for f in /proc/sys/net/ipv4/conf/*/rp_filter; do
    echo 0 > "$f" || true
done

# Policy routing for TPROXY (fwmark 0x2 → table 102 → local)
echo "[gate-init] Configuring policy routing (Table 102, Mark 0x2)..."
ip rule del fwmark 0x2 table 102 2>/dev/null || true
ip rule add fwmark 0x2 table 102
ip route flush table 102 2>/dev/null || true
ip route add local default dev lo table 102

# Remove previous polis table (preserve Docker's internal DNS rules)
echo "[gate-init] Applying nftables ruleset..."
nft delete table inet polis 2>/dev/null || true

# Apply consolidated nftables ruleset
nft -f - <<EOF
table inet polis {
    # TPROXY interception (mangle priority)
    chain prerouting_tproxy {
        type filter hook prerouting priority mangle; policy accept;
        
        # Defense-in-depth: drop IPv6 early
        meta nfproto ipv6 drop
        
        # Skip gate's own traffic (IPv4 only from here)
        ip saddr { $LOCAL_IPS } return
        ip daddr 127.0.0.0/8 return
        meta mark 0x2 return
        
        # Skip internal service subnets (no proxying internal-to-internal)
        ip daddr { $INTERNAL_SUBNET, $GATEWAY_SUBNET, $EXTERNAL_SUBNET } return
        
        # Intercept all TCP via TPROXY
        tcp dport 1-65535 tproxy to :18080 meta mark set 0x2 accept
    }

    # DNS redirection (force all DNS through CoreDNS)
    chain prerouting_dnat {
        type nat hook prerouting priority dstnat; policy accept;
        iifname "$INTERNAL_IF" udp dport 53 dnat ip to $RESOLVER_IP
        iifname "$INTERNAL_IF" tcp dport 53 dnat ip to $RESOLVER_IP
    }

    # Zero-trust forward: only DNS DNAT, drop everything else
    chain forward {
        type filter hook forward priority filter; policy drop;
        ct state established,related accept
        ip daddr $RESOLVER_IP udp dport 53 accept
        ip daddr $RESOLVER_IP tcp dport 53 accept
        log prefix "[polis-drop] " limit rate 5/minute counter drop
    }

    # Input: defense-in-depth IPv6 drop
    chain input {
        type filter hook input priority filter; policy accept;
        meta nfproto ipv6 drop
    }
}
EOF

# Enable route_localnet (required for TPROXY)
echo "[gate-init] Enabling localnet routing..."
sysctl -w net.ipv4.conf.all.route_localnet=1 || true
sysctl -w net.ipv4.conf.default.route_localnet=1 || true
sysctl -w "net.ipv4.conf.$INTERNAL_IF.route_localnet=1" || true
sysctl -w "net.ipv4.conf.$INTERNAL_IF.rp_filter=0" || true

echo "[gate-init] Networking setup complete."
