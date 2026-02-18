#!/bin/bash
# =============================================================================
# polis-mcp-call: MCP JSON-RPC caller for polis-toolbox
# =============================================================================
# Streamable HTTP protocol: initialize → notify → tools/call
#
# Usage: polis-mcp-call <tool_name> '<json_arguments>'
# =============================================================================
set -euo pipefail

# Toolbox runs HTTPS (TLS cert signed by Polis CA, trusted by workspace)
MCP_URL="${POLIS_MCP_URL:-https://toolbox:8080/mcp}"
TOOL_NAME="${1:?Usage: polis-mcp-call <tool_name> '<json_arguments>'}"
TOOL_ARGS="${2:-\{\}}"

HDR_FILE=$(mktemp /tmp/mcp-hdr.XXXXXX)
trap 'rm -f "$HDR_FILE"' EXIT

_curl() {
    curl -s -f --max-time 15 \
        -H "Content-Type: application/json" \
        -H "Accept: application/json, text/event-stream" \
        "$@" 2>/dev/null
}

# Step 1: Initialize — capture session ID
_curl -X POST "$MCP_URL" \
    -D "$HDR_FILE" -o /dev/null \
    -d '{"jsonrpc":"2.0","method":"initialize","id":1,"params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"polis-cli","version":"1.0"}}}'

SID=$(grep -i '^mcp-session-id:' "$HDR_FILE" | tr -d '\r' | awk '{print $2}')
if [[ -z "$SID" ]]; then
    echo '{"error":"toolbox unreachable or not ready"}' >&2
    exit 1
fi

# Step 2: Send initialized notification
_curl -X POST "$MCP_URL" \
    -H "mcp-session-id: $SID" \
    -d '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    -o /dev/null || true

# Step 3: Call the tool — parse SSE stream for the result line
RESULT=$(_curl -X POST "$MCP_URL" \
    -H "mcp-session-id: $SID" \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"id\":2,\"params\":{\"name\":\"${TOOL_NAME}\",\"arguments\":${TOOL_ARGS}}}" \
    | grep '^data: ' \
    | sed 's/^data: //' \
    | grep -v '^$' \
    | tail -1)

if [[ -z "$RESULT" ]]; then
    echo '{"error":"no response from toolbox"}' >&2
    exit 1
fi

# Unwrap MCP envelope: extract text content from result.content[0].text
if command -v jq &>/dev/null; then
    TEXT=$(echo "$RESULT" | jq -r '.result.content[0].text // .error.message // empty' 2>/dev/null)
    if [[ -n "$TEXT" ]]; then
        echo "$TEXT"
        exit 0
    fi
fi

echo "$RESULT"
