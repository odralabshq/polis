#!/bin/bash
set -euo pipefail

# Check g3proxy process
if ! pgrep -x g3proxy > /dev/null; then
    echo "UNHEALTHY: g3proxy not running"
    exit 1
fi

# Check certgen sidecar is reachable (g3fcgen now runs in certgen container)
if ! timeout 1 bash -c "echo > /dev/udp/certgen/2999" 2>/dev/null; then
    echo "UNHEALTHY: certgen sidecar not reachable at certgen:2999"
    exit 1
fi

# Check ICAP echo service via OPTIONS request (verifies ICAP protocol, not just TCP)
# head -1 closes the pipe after reading the first line, causing nc to exit via SIGPIPE
# || true prevents set -e from aborting if nc/head pipeline fails
ICAP_ECHO=$(printf 'OPTIONS icap://sentinel:1344/echo ICAP/1.0\r\nHost: sentinel\r\n\r\n' | \
            timeout 3 nc sentinel 1344 2>/dev/null | head -1 || true)
if [[ "$ICAP_ECHO" != *"200"* ]]; then
    echo "UNHEALTHY: ICAP echo service not responding"
    exit 1
fi

# Check ICAP squidclamav service via OPTIONS (verifies ClamAV integration)
ICAP_AV=$(printf 'OPTIONS icap://sentinel:1344/squidclamav ICAP/1.0\r\nHost: sentinel\r\n\r\n' | \
          timeout 3 nc sentinel 1344 2>/dev/null | head -1 || true)
if [[ "$ICAP_AV" != *"200"* ]]; then
    echo "UNHEALTHY: ICAP squidclamav service not responding (ClamAV may be down)"
    exit 1
fi

# Check ICAP credcheck (DLP) service via OPTIONS (verifies REQMOD path)
ICAP_DLP=$(printf 'OPTIONS icap://sentinel:1344/credcheck ICAP/1.0\r\nHost: sentinel\r\n\r\n' | \
           timeout 3 nc sentinel 1344 2>/dev/null | head -1 || true)
if [[ "$ICAP_DLP" != *"200"* ]]; then
    echo "UNHEALTHY: ICAP credcheck service not responding (DLP module may be loading)"
    exit 1
fi

# Check nftables ruleset is loaded
if ! nft list ruleset > /dev/null 2>&1; then
    echo "UNHEALTHY: nftables ruleset not functional"
    exit 1
fi

echo "OK"
exit 0
