#!/bin/bash
# =============================================================================
# polis-toolbox-call: JSON-RPC caller for the polis-toolbox HITL service
# =============================================================================
# Streamable HTTP protocol: initialize → notify → tools/call
#
# Usage: polis-toolbox-call <tool_name> '<json_arguments>'
# =============================================================================
set -euo pipefail

# Toolbox runs HTTPS (TLS cert signed by Polis CA, trusted by workspace)
MCP_URL="${POLIS_TOOLBOX_URL:-https://toolbox:8080/mcp}"
TOOL_NAME="${1:?Usage: polis-toolbox-call <tool_name> '<json_arguments>'}"
TOOL_ARGS="${2:-\{\}}"

HDR_FILE=$(mktemp /tmp/mcp-hdr.XXXXXX)
trap 'rm -f "$HDR_FILE"' EXIT

_curl() {
    curl -s -f --max-time 15 \
        -H "Content-Type: application/json" \
        -H "Accept: application/json, text/event-stream" \
        "$@" 2>/dev/null
}

_error_json() {
    local msg="$1"
    printf '{"error":"%s","url":"%s"}\n' "$msg" "$MCP_URL"
}

# Step 1: Initialize — capture session ID (retry up to 3 times for cold starts)
SID=""
for _attempt in 1 2 3; do
    _curl -X POST "$MCP_URL" \
        -D "$HDR_FILE" -o /dev/null \
        -d '{"jsonrpc":"2.0","method":"initialize","id":1,"params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"polis-cli","version":"1.0"}}}' \
        || true
    SID=$(grep -i '^mcp-session-id:' "$HDR_FILE" 2>/dev/null | tr -d '\r' | awk '{print $2}' || true)
    [[ -n "$SID" ]] && break
    sleep 2
done

if [[ -z "$SID" ]]; then
    _error_json "toolbox unreachable or not ready"
    exit 1
fi

# Step 2: Send initialized notification
_curl -X POST "$MCP_URL" \
    -H "mcp-session-id: $SID" \
    -d '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    -o /dev/null || true

# Step 3: Call the tool — parse SSE stream for the result line
RAW_RESULT=$(_curl -X POST "$MCP_URL" \
    -H "mcp-session-id: $SID" \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"id\":2,\"params\":{\"name\":\"${TOOL_NAME}\",\"arguments\":${TOOL_ARGS}}}" \
    || true)

RESULT=$(printf '%s\n' "$RAW_RESULT" \
    | grep '^data: ' \
    | sed 's/^data: //' \
    | grep -v '^$' \
    | tail -1 \
    || true)

if [[ -z "$RESULT" ]]; then
    _error_json "no response from toolbox"
    exit 1
fi

# Unwrap MCP envelope: extract .result.content[0].text from JSON-RPC response.
# Use jq if available, otherwise fall back to sed/grep (workspace image may lack jq).
_unwrap() {
    if command -v jq &>/dev/null; then
        jq -r '.result.content[0].text // .error.message // empty' 2>/dev/null
    else
        # Extract the "text" field value from the MCP envelope using sed.
        # The value is JSON-escaped, so unescape \" → " and \\ → \ afterwards.
        sed -n 's/.*"text":"\(.*\)"}],"isError":.*/\1/p' \
            | sed 's/\\"/"/g; s/\\\\/\\/g'
    fi
}

TEXT=$(echo "$RESULT" | _unwrap)
if [[ -n "$TEXT" ]]; then
    echo "$TEXT"
else
    echo "$RESULT"
fi
