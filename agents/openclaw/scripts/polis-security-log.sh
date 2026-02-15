#!/bin/bash
# Get recent security event log
# Usage: polis-security-log
set -euo pipefail
SCRIPT_DIR="$(dirname "$(readlink -f "$0")")"
exec "$SCRIPT_DIR/polis-mcp-call.sh" get_security_log "{}"
