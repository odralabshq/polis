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
echo "[gate-init] Configuring policy routing..."
# Use table 100 for TPROXY
# Some kernels/ip versions prefer 'table' and might fail if not careful
ip rule del fwmark 0x1 table 100 2>/dev/null || true
ip rule add fwmark 0x1 table 100

ip route flush table 100 2>/dev/null || true
ip route add local default dev lo table 100

# 4. TPROXY Iptables Rules
echo "[gate-init] Configuring TPROXY nftables/iptables..."

# Create G3TPROXY chain in mangle table
iptables -t mangle -N G3TPROXY 2>/dev/null || true
iptables -t mangle -F G3TPROXY

# 1. MARK established connections to bypass TPROXY
iptables -t mangle -A G3TPROXY -m socket --transparent -j MARK --set-xmark 0x1/0xffffffff
iptables -t mangle -A G3TPROXY -m socket --transparent -j RETURN

# 2. TPROXY redirect for HTTP/HTTPS (10.10.1.0/24 -> any)
# Redirect to g3proxy on port 18080
iptables -t mangle -A G3TPROXY -p tcp -j TPROXY --on-port 18080 --tproxy-mark 0x1/0xffffffff

# Apply G3TPROXY to incoming traffic from internal subnet
iptables -t mangle -D PREROUTING -i "$INTERNAL_IF" -s 10.10.1.0/24 -p tcp -j G3TPROXY 2>/dev/null || true
iptables -t mangle -A PREROUTING -i "$INTERNAL_IF" -s 10.10.1.0/24 -p tcp -j G3TPROXY

# 5. DNS Redirection
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
