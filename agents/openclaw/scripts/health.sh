#!/bin/bash
# OpenClaw health check script
set -euo pipefail

GATEWAY_PORT="${OPENCLAW_GATEWAY_PORT:-18789}"
GATEWAY_URL="http://127.0.0.1:${GATEWAY_PORT}"

# Check if gateway is responding
if curl -sf "${GATEWAY_URL}/health" >/dev/null 2>&1; then
    echo "OpenClaw gateway healthy"
    exit 0
fi

# Fallback: check if process is running
if pgrep -f "node.*gateway" >/dev/null; then
    echo "OpenClaw gateway process running (health endpoint not ready)"
    exit 0
fi

echo "OpenClaw gateway not healthy"
exit 1
