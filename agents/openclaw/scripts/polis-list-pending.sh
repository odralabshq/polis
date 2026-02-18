#!/bin/bash
# List all pending approval requests
# Usage: polis-list-pending
set -euo pipefail
SCRIPT_DIR="$(dirname "$(readlink -f "$0")")"
exec "$SCRIPT_DIR/polis-mcp-call.sh" list_pending_approvals "{}"
