#!/bin/bash
set -euo pipefail

# Check g3proxy process
if ! pgrep -x g3proxy > /dev/null; then
    echo "UNHEALTHY: g3proxy not running"
    exit 1
fi

# Check g3fcgen process
if ! pgrep -x g3fcgen > /dev/null; then
    echo "UNHEALTHY: g3fcgen not running"
    exit 1
fi

# Check TPROXY iptables rules
if ! iptables -t mangle -S G3TPROXY 2>/dev/null | grep -q "TPROXY"; then
    echo "UNHEALTHY: TPROXY not configured"
    exit 1
fi

# Check ICAP echo service via OPTIONS request (verifies ICAP protocol, not just TCP)
ICAP_ECHO=$(printf 'OPTIONS icap://icap:1344/echo ICAP/1.0\r\nHost: icap\r\n\r\n' | \
            timeout 3 nc icap 1344 2>/dev/null | head -1)
if [[ "$ICAP_ECHO" != *"200"* ]]; then
    echo "UNHEALTHY: ICAP echo service not responding"
    exit 1
fi

# Check ICAP squidclamav service via OPTIONS (verifies ClamAV integration)
ICAP_AV=$(printf 'OPTIONS icap://icap:1344/squidclamav ICAP/1.0\r\nHost: icap\r\n\r\n' | \
          timeout 5 nc icap 1344 2>/dev/null | head -1)
if [[ "$ICAP_AV" != *"200"* ]]; then
    echo "UNHEALTHY: ICAP squidclamav service not responding (ClamAV may be down)"
    exit 1
fi

echo "OK"
exit 0
