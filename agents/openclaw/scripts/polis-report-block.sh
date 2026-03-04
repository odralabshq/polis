#!/bin/bash
# Report a blocked request to the polis security system
# Usage: polis-report-block <request_id> <reason> <destination> [pattern]
set -euo pipefail
SCRIPT_DIR="$(dirname "$(readlink -f "$0")")"
REQ_ID="${1:?Usage: polis-report-block <request_id> <reason> <destination> [pattern]}"
RAW_REASON="${2:?Missing reason}"
DEST="${3:?Missing destination}"
PATTERN="${4:-}"

# Normalize DLP-side reasons to toolbox enum values.
case "$RAW_REASON" in
    new_domain_prompt|new_domain_blocked)
        REASON="url_blocked"
        ;;
    *)
        REASON="$RAW_REASON"
        ;;
esac

ARGS="{\"request_id\":\"${REQ_ID}\",\"reason\":\"${REASON}\",\"destination\":\"${DEST}\""
[[ -n "$PATTERN" ]] && ARGS="${ARGS},\"pattern\":\"${PATTERN}\""
ARGS="${ARGS}}"

exec "$SCRIPT_DIR/polis-toolbox-call.sh" report_block "$ARGS"
