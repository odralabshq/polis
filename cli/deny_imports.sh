#!/usr/bin/env bash
# deny_imports.sh — CI boundary check for Clean Architecture layer isolation.
#
# Enforces four-layer dependency rules:
#   domain/     → no imports from application/, infra/, commands/, output/
#               → no async fn declarations
#               → no imports from tokio, std::fs, std::process, std::net
#   application/ → no imports from infra/, commands/, output/
#               → no std::fs, std::process::Command, std::net usage
#               → no crate::workspace:: imports
#   infra/      → no imports from commands/ or output/
#               → no println!/eprintln! outside #[cfg(test)]
#
# Global checks (all source files):
#   → no old root-level module imports (crate::command_runner, crate::provisioner,
#     crate::state, crate::ssh, crate::assets) — these modules were deleted
#
# Exceptions:
#   → internal.rs::ssh_proxy() is the only documented exception for
#     std::process::Command in the application layer (excluded by path)
#
# Usage: bash cli/deny_imports.sh
# Exit: 0 = no violations, 1 = violations found

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC_DIR="$SCRIPT_DIR/src"

VIOLATIONS=0

# Helper: grep non-comment lines for a pattern in a directory
check_pattern() {
    local dir="$1"
    local pattern="$2"
    local message="$3"

    if [ ! -d "$dir" ]; then
        return 0
    fi

    while IFS= read -r -d '' file; do
        # Strip comment lines before grepping
        if grep -v '^\s*//' "$file" | grep -v '^\s*\*' | grep -qE "$pattern"; then
            echo "VIOLATION: $message"
            grep -v '^\s*//' "$file" | grep -v '^\s*\*' | grep -nE "$pattern" | while IFS= read -r line; do
                echo "  $file: $line"
            done
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    done < <(find "$dir" -name "*.rs" -print0)
}

# Helper: grep non-comment lines for a pattern in a directory, excluding a file pattern
check_pattern_excluding() {
    local dir="$1"
    local pattern="$2"
    local message="$3"
    local exclude_pattern="$4"

    if [ ! -d "$dir" ]; then
        return 0
    fi

    while IFS= read -r -d '' file; do
        # Skip excluded files
        if echo "$file" | grep -qE "$exclude_pattern"; then
            continue
        fi
        if grep -v '^\s*//' "$file" | grep -v '^\s*\*' | grep -qE "$pattern"; then
            echo "VIOLATION: $message"
            grep -v '^\s*//' "$file" | grep -v '^\s*\*' | grep -nE "$pattern" | while IFS= read -r line; do
                echo "  $file: $line"
            done
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    done < <(find "$dir" -name "*.rs" -print0)
}

echo "=== deny_imports.sh: Checking architectural boundaries ==="
echo ""

# ── domain/ checks ────────────────────────────────────────────────────────────

DOMAIN_DIR="$SRC_DIR/domain"

echo "Checking domain/ layer..."

check_pattern "$DOMAIN_DIR" \
    "crate::(application|infra|commands|output)" \
    "domain/ must not import from application/, infra/, commands/, or output/"

check_pattern "$DOMAIN_DIR" \
    "^[[:space:]]*(pub[[:space:]]+)?async[[:space:]]+fn" \
    "domain/ must not contain async fn declarations"

check_pattern "$DOMAIN_DIR" \
    "use tokio::|extern crate tokio" \
    "domain/ must not import from tokio"

check_pattern "$DOMAIN_DIR" \
    "use std::fs::|std::fs::" \
    "domain/ must not import from std::fs"

check_pattern "$DOMAIN_DIR" \
    "use std::process::|std::process::" \
    "domain/ must not import from std::process"

check_pattern "$DOMAIN_DIR" \
    "use std::net::|std::net::" \
    "domain/ must not import from std::net"

# ── application/ checks ───────────────────────────────────────────────────────

APP_DIR="$SRC_DIR/application"

echo "Checking application/ layer..."

check_pattern "$APP_DIR" \
    "crate::(infra|commands|output)" \
    "application/ must not import from infra/, commands/, or output/"

check_pattern "$APP_DIR" \
    "crate::workspace::" \
    "application/ must not import from crate::workspace (module deleted)"

# std::process::Command in application/ — internal.rs::ssh_proxy() is the only exception
check_pattern_excluding "$APP_DIR" \
    "std::process::Command" \
    "application/ must not use std::process::Command (use CommandRunner port instead)" \
    "internal\.rs"

check_pattern "$APP_DIR" \
    "use std::net::|std::net::TcpStream|std::net::ToSocketAddrs" \
    "application/ must not use std::net directly (use NetworkProbe port instead)"

check_pattern "$APP_DIR" \
    "use std::fs::|[^a-z]std::fs::" \
    "application/ must not use std::fs directly (use ports or spawn_blocking)"

# ── infra/ checks ─────────────────────────────────────────────────────────────

INFRA_DIR="$SRC_DIR/infra"

echo "Checking infra/ layer..."

check_pattern "$INFRA_DIR" \
    "crate::(commands|output)" \
    "infra/ must not import from commands/ or output/"

# Check for println!/eprintln! outside #[cfg(test)] in infra/
while IFS= read -r -d '' file; do
    in_test_block=false
    while IFS= read -r line; do
        trimmed="${line#"${line%%[![:space:]]*}"}"
        if [[ "$trimmed" == "#[cfg(test)]"* ]]; then
            in_test_block=true
        fi
        if [[ "$in_test_block" == false ]] && \
           [[ "$trimmed" != "//"* ]] && \
           echo "$line" | grep -qE "(println!|eprintln!)"; then
            echo "VIOLATION: infra/ must not use println!/eprintln! outside #[cfg(test)]"
            echo "  $file: $line"
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    done < "$file"
done < <(find "$INFRA_DIR" -name "*.rs" -print0 2>/dev/null)

# ── Global checks — old root-level module imports ─────────────────────────────

echo "Checking for deleted root-level module imports..."

# These modules were deleted; any remaining imports are bugs.
# Exclude infra/ self-references (e.g. infra/command_runner.rs declaring itself).
for old_mod in "command_runner" "provisioner" "state" "ssh" "assets"; do
    while IFS= read -r -d '' file; do
        # Normalize path separators
        norm_file="${file//\\//}"
        # Allow infra/ files to reference their own module path in comments/docs
        if echo "$norm_file" | grep -q "/infra/"; then
            continue
        fi
        if grep -v '^\s*//' "$file" | grep -v '^\s*\*' | grep -qE "crate::${old_mod}::"; then
            echo "VIOLATION: old root-level module 'crate::${old_mod}::' still referenced (module deleted)"
            grep -v '^\s*//' "$file" | grep -v '^\s*\*' | grep -nE "crate::${old_mod}::" | while IFS= read -r line; do
                echo "  $file: $line"
            done
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    done < <(find "$SRC_DIR" -name "*.rs" -print0)
done

echo ""

if [ "$VIOLATIONS" -eq 0 ]; then
    echo "✓ All architectural boundaries are clean."
    exit 0
else
    echo "✗ Found $VIOLATIONS boundary violation(s). Fix before merging."
    exit 1
fi
