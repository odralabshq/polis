#!/bin/bash
set -euo pipefail

# =============================================================================
# Polis Gateway Networking Setup (Root required)
# =============================================================================

echo "[gate-init] Starting networking setup..."

# 1. Disable IPv6 (Try, but don't fail)
if [ -f /proc/sys/net/ipv6/conf/all/disable_ipv6 ]; then
    echo "[gate-init] Attempting to disable IPv6 via sysctl..."
    sysctl -w net.ipv6.conf.all.disable_ipv6=1 || true
    sysctl -w net.ipv6.conf.default.disable_ipv6=1 || true
fi

# Disable rp_filter (Required for TPROXY, especially on WSL2/Docker)
echo "[gate-init] Disabling rp_filter..."
for f in /proc/sys/net/ipv4/conf/*/rp_filter; do
    echo 0 > "$f" || true
done

# Block IPv6 via iptables regardless (defense in depth)
ip6tables -P INPUT DROP 2>/dev/null || true
ip6tables -P FORWARD DROP 2>/dev/null || true
ip6tables -P OUTPUT DROP 2>/dev/null || true

# 2. Detect internal interface (bridge connected to workspace)
echo "[gate-init] Detecting internal interface..."
# We expect 10.10.1.0/24 to be on eth0 or eth1
# Scan routes for the internal subnet
INTERNAL_IF=$(ip -4 route show 10.10.1.0/24 | awk '{print $3}' | head -n 1)

if [ -z "$INTERNAL_IF" ]; then
    echo "[gate-init] ERROR: Could not detect internal interface for 10.10.1.0/24"
    exit 1
fi
echo "[gate-init] Detected internal interface: $INTERNAL_IF"

# 3. Transparent Proxy (TPROXY) Routing
echo "[gate-init] Configuring policy routing (Table 102, Mark 0x2)..."
ip rule del fwmark 0x2 table 102 2>/dev/null || true
ip rule add fwmark 0x2 table 102
ip route flush table 102 2>/dev/null || true
ip route add local default dev lo table 102

# 4. TPROXY Iptables Rules
echo "[gate-init] Configuring Universal TPROXY Interceptor..."

# Create G3TPROXY chain in mangle table
iptables -t mangle -N G3TPROXY 2>/dev/null || true
iptables -t mangle -F G3TPROXY

# 1. Skip local gate traffic (don't proxy g3proxy's own outbound traffic)
# We skip all IPs assigned to this container's interfaces
for ip in $(ip -4 addr show | grep inet | awk '{print $2}' | cut -d/ -f1); do
    iptables -t mangle -A G3TPROXY -s "$ip" -j RETURN
done

# 2. Skip loopback
iptables -t mangle -A G3TPROXY -d 127.0.0.0/8 -j RETURN

# 3. Skip already marked packets (avoid loops)
iptables -t mangle -A G3TPROXY -m mark --mark 0x2 -j RETURN

# 4. Skip internal service subnets (gateway-bridge and external-bridge)
# Traffic to ICAP/Sentinal (10.30.x.x) should NOT be intercepted by the proxy
iptables -t mangle -A G3TPROXY -d 10.30.1.0/24 -j RETURN
iptables -t mangle -A G3TPROXY -d 10.20.1.0/24 -j RETURN

# 5. TPROXY Interception
# Redirect ALL remaining TCP traffic to g3proxy
iptables -t mangle -A G3TPROXY -p tcp -j TPROXY --on-port 18080 --tproxy-mark 0x2/0xffffffff

# Apply Universal Interception to ALL PREROUTING traffic
iptables -t mangle -D PREROUTING -p tcp -j G3TPROXY 2>/dev/null || true
iptables -t mangle -A PREROUTING -p tcp -j G3TPROXY

# 5. Fix Checksums (Crucial for TPROXY on WSL2/Docker)
echo "[gate-init] Enabling checksum fill (WSL2 Fix)..."
iptables -t mangle -D POSTROUTING -p tcp -j CHECKSUM --checksum-fill 2>/dev/null || true
iptables -t mangle -A POSTROUTING -p tcp -j CHECKSUM --checksum-fill

# 6. Enable routing to localnet (Needed for some TPROXY setups)
sysctl -w net.ipv4.conf.all.route_localnet=1 || true
sysctl -w net.ipv4.conf.eth1.route_localnet=1 || true

# 7. DNS Redirection
echo "[gate-init] Redirecting DNS to resolver (10.30.1.10)..."
iptables -t nat -D PREROUTING -i "$INTERNAL_IF" -p udp --dport 53 -j DNAT --to-destination 10.30.1.10 2>/dev/null || true
iptables -t nat -A PREROUTING -i "$INTERNAL_IF" -p udp --dport 53 -j DNAT --to-destination 10.30.1.10
iptables -t nat -D PREROUTING -i "$INTERNAL_IF" -p tcp --dport 53 -j DNAT --to-destination 10.30.1.10 2>/dev/null || true
iptables -t nat -A PREROUTING -i "$INTERNAL_IF" -p tcp --dport 53 -j DNAT --to-destination 10.30.1.10

# 6. Outbound NAT (for non-filtered traffic)
echo "[gate-init] Configuring MASQUERADE for outbound traffic..."
# Identify external interface (the one with the default route)
EXTERNAL_IF=$(ip route show default | awk '{print $5}' | head -n 1)
if [ -n "$EXTERNAL_IF" ]; then
    iptables -t nat -D POSTROUTING -s 10.10.1.0/24 -o "$EXTERNAL_IF" -j MASQUERADE 2>/dev/null || true
    iptables -t nat -A POSTROUTING -s 10.10.1.0/24 -o "$EXTERNAL_IF" -j MASQUERADE
fi

echo "[gate-init] Networking setup complete."
