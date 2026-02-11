#!/bin/bash
set -euo pipefail

INPUT_FILE="${1:-/etc/c-icap/blocklist.txt}"
MIN_ENTRIES=5

if [[ ! -f "$INPUT_FILE" ]]; then
    echo "[blocklist] CRITICAL: File not found" >&2
    exit 1
fi

ENTRY_COUNT=$(grep -cv '^#\|^$' "$INPUT_FILE" 2>/dev/null || true)

if [[ "$ENTRY_COUNT" -lt "$MIN_ENTRIES" ]]; then
    echo "[blocklist] CRITICAL: Only $ENTRY_COUNT entries (min: $MIN_ENTRIES)" >&2
    exit 1
fi

echo "[blocklist] Valid: $ENTRY_COUNT domains"
