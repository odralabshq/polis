#!/bin/bash
# =============================================================================
# polis-mcp-call: MCP JSON-RPC caller for polis-toolbox
# =============================================================================
# Streamable HTTP + SSE protocol: initialize → notify → tools/call
#
# Usage: polis-mcp-call <tool_name> '<json_arguments>'
# =============================================================================
set -euo pipefail

MCP_URL="${POLIS_MCP_URL:-http://toolbox:8080/mcp}"
TOOL_NAME="${1:?Usage: polis-mcp-call <tool_name> '<json_arguments>'}"
TOOL_ARGS="${2:-\{\}}"

HDR_FILE=$(mktemp /tmp/mcp-hdr.XXXXXX)
trap 'rm -f "$HDR_FILE"' EXIT

# Step 1: Initialize — capture session ID
curl -s -m 15 -X POST "$MCP_URL" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json, text/event-stream" \
    -D "$HDR_FILE" -o /dev/null \
    -d '{"jsonrpc":"2.0","method":"initialize","id":1,"params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"polis-cli","version":"1.0"}}}' 2>/dev/null

SID=$(grep -i '^mcp-session-id:' "$HDR_FILE" | tr -d '\r' | awk '{print $2}')
if [ -z "$SID" ]; then
    echo '{"error":"Failed to get MCP session"}' >&2
    exit 1
fi

# Step 2: Send initialized notification
curl -s -m 5 -X POST "$MCP_URL" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json, text/event-stream" \
    -H "mcp-session-id: $SID" \
    -d '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    -o /dev/null 2>/dev/null

# Step 3: Call the tool
RESULT=$(curl -s -m 15 -X POST "$MCP_URL" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json, text/event-stream" \
    -H "mcp-session-id: $SID" \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"id\":2,\"params\":{\"name\":\"${TOOL_NAME}\",\"arguments\":${TOOL_ARGS}}}" 2>/dev/null \
    | grep '^data: ' \
    | sed 's/^data: //' \
    | grep -v '^$' \
    | tail -1)

if [ -z "$RESULT" ]; then
    echo '{"error":"No response from MCP server"}' >&2
    exit 1
fi

# Extract text content from MCP envelope
if command -v jq &>/dev/null; then
    TEXT=$(echo "$RESULT" | jq -r '.result.content[0].text // .error.message // empty' 2>/dev/null)
    if [ -n "$TEXT" ]; then
        echo "$TEXT"
        exit 0
    fi
fi

echo "$RESULT"
