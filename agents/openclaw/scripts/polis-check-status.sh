#!/bin/bash
# Check the approval status of a blocked request
# Usage: polis-check-status <request_id>
set -euo pipefail
SCRIPT_DIR="$(dirname "$(readlink -f "$0")")"
REQ_ID="${1:?Usage: polis-check-status <request_id>}"
exec "$SCRIPT_DIR/polis-mcp-call.sh" check_request_status "{\"request_id\":\"${REQ_ID}\"}"
