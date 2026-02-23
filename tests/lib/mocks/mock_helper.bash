#!/usr/bin/env bash
# Lightweight command mock with call tracking. Zero external dependencies.
# Security: validates inputs before eval to prevent CWE-78 injection.

_MOCK_CALLS=()

mock_command() {
    local cmd="$1" output="${2:-}" rc="${3:-0}"
    [[ "$cmd" =~ ^[a-zA-Z_][a-zA-Z0-9_]*$ ]] || { echo "mock_command: invalid name '$cmd'" >&2; return 1; }
    [[ "$rc" =~ ^[0-9]+$ ]]                    || { echo "mock_command: invalid rc '$rc'" >&2; return 1; }
    eval "${cmd}() { _MOCK_CALLS+=(\"\$*\"); cat <<'_MOCK_EOF_'
${output}
_MOCK_EOF_
return ${rc}; }"
    export -f "${cmd}"
}

mock_call_count() {
    local n=0
    for c in "${_MOCK_CALLS[@]}"; do
        [[ "$c" == "$1"* ]] && ((n++))
    done
    echo "$n"
}

mock_call_args() { echo "${_MOCK_CALLS[$1]:-}"; }
mock_reset()     { _MOCK_CALLS=(); }
