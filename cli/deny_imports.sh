#!/usr/bin/env bash
# deny_imports.sh — CI boundary check for Clean Architecture layer isolation.
#
# Enforces four-layer dependency rules:
#   domain/     → no imports from application/, infra/, commands/, output/
#               → no async fn declarations
#               → no imports from tokio, std::fs, std::process, std::net
#   application/ → no imports from infra/, commands/, output/
#   infra/      → no imports from commands/ or output/
#               → no println!/eprintln! outside #[cfg(test)]
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

# ── infra/ checks ─────────────────────────────────────────────────────────────

INFRA_DIR="$SRC_DIR/infra"

echo "Checking infra/ layer..."

check_pattern "$INFRA_DIR" \
    "crate::(commands|output)" \
    "infra/ must not import from commands/ or output/"

# Check for println!/eprintln! outside #[cfg(test)] in infra/
# This is a simplified check — it flags any println!/eprintln! in infra/
# (the Rust structural test does the more precise cfg(test) scoping)
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

echo ""

if [ "$VIOLATIONS" -eq 0 ]; then
    echo "✓ All architectural boundaries are clean."
    exit 0
else
    echo "✗ Found $VIOLATIONS boundary violation(s). Fix before merging."
    exit 1
fi
