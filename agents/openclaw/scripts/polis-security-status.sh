#!/bin/bash
# Get current security system status
# Usage: polis-security-status
set -euo pipefail
SCRIPT_DIR="$(dirname "$(readlink -f "$0")")"
exec "$SCRIPT_DIR/polis-mcp-call.sh" get_security_status "{}"
