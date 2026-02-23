#!/bin/bash
# Get recent security event log
# Usage: polis-security-log
set -euo pipefail
SCRIPT_DIR="$(dirname "$(readlink -f "$0")")"
exec "$SCRIPT_DIR/polis-toolbox-call.sh" get_security_log "{}"
