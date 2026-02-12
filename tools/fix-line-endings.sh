#!/bin/bash
# fix-line-endings.sh — Convert CRLF → LF for all shell scripts and config
# files that WSL/Linux needs to execute or parse.
#
# Run from WSL:  bash /mnt/c/Users/adam/Desktop/startup/polis/polis/tools/fix-line-endings.sh
# Or from the polis/tools directory:  bash fix-line-endings.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

converted=0
bom_fixed=0
skipped=0

fix_file() {
    local f="$1"
    local changed=0
    if [[ ! -f "$f" ]]; then
        return
    fi

    # Strip UTF-8 BOM (EF BB BF) if present
    if head -c3 "$f" | grep -qP '\xef\xbb\xbf' 2>/dev/null; then
        # Remove BOM: skip first 3 bytes
        tail -c +4 "$f" > "${f}.tmp" && mv "${f}.tmp" "$f"
        echo -e "  ${YELLOW}bom${NC}    $f"
        bom_fixed=$((bom_fixed + 1))
        changed=1
    fi

    # Fix CRLF → LF
    if grep -qP '\r$' "$f" 2>/dev/null; then
        sed -i 's/\r$//' "$f"
        echo -e "  ${GREEN}crlf${NC}   $f"
        converted=$((converted + 1))
        changed=1
    fi

    if [[ $changed -eq 0 ]]; then
        skipped=$((skipped + 1))
    fi
}

echo "=== Polis: Fixing CRLF → LF line endings ==="
echo "Project root: ${PROJECT_ROOT}"
echo ""

# 1. Main CLI script
fix_file "${PROJECT_ROOT}/tools/polis.sh"

# 2. All scripts in polis/scripts/
for f in "${PROJECT_ROOT}"/scripts/*.sh; do
    fix_file "$f"
done

# 3. Agent scripts and configs
for agent_dir in "${PROJECT_ROOT}"/agents/*/; do
    [[ -d "$agent_dir" ]] || continue
    for f in "$agent_dir"/*.sh "$agent_dir"/*.conf; do
        fix_file "$f"
    done
    for f in "$agent_dir"/scripts/*.sh; do
        fix_file "$f"
    done
    for f in "$agent_dir"/config/*.service "$agent_dir"/config/*.conf; do
        fix_file "$f"
    done
done

# 4. Dockerfiles (heredoc scripts inside get built, but good to fix anyway)
for f in "${PROJECT_ROOT}"/build/*/Dockerfile "${PROJECT_ROOT}"/build/*/Dockerfile.*; do
    fix_file "$f"
done

# 5. Config files parsed by Linux tools
for f in "${PROJECT_ROOT}"/config/*.conf "${PROJECT_ROOT}"/config/*.yaml \
         "${PROJECT_ROOT}"/config/*.yml; do
    fix_file "$f"
done

# 6. Docker compose
fix_file "${PROJECT_ROOT}/docker-compose.yml"

# 7. .env file
fix_file "${PROJECT_ROOT}/.env"

# 8. Secrets files (ACL, password files — CRLF breaks Valkey ACL parser)
for f in "${PROJECT_ROOT}"/secrets/*.acl "${PROJECT_ROOT}"/secrets/*.txt; do
    fix_file "$f"
done

# 9. Test files (BATS tests, helpers)
for f in "${PROJECT_ROOT}"/tests/*.sh "${PROJECT_ROOT}"/tests/*.bash; do
    fix_file "$f"
done
for f in "${PROJECT_ROOT}"/tests/helpers/*.bash; do
    fix_file "$f"
done
for f in "${PROJECT_ROOT}"/tests/unit/*.bats "${PROJECT_ROOT}"/tests/integration/*.bats \
         "${PROJECT_ROOT}"/tests/e2e/*.bats; do
    fix_file "$f"
done

# 10. C source files (ICAP modules)
for f in "${PROJECT_ROOT}"/build/icap/*.c; do
    fix_file "$f"
done

# 11. Rust source files (MCP agent, CLI)
find "${PROJECT_ROOT}/crates" -type f \( -name "*.rs" -o -name "*.toml" \) 2>/dev/null | while read -r f; do
    fix_file "$f"
done

# 12. Cargo workspace files
fix_file "${PROJECT_ROOT}/Cargo.toml"
fix_file "${PROJECT_ROOT}/Cargo.lock"

# 13. GitHub workflows
for f in "${PROJECT_ROOT}"/.github/workflows/*.yml "${PROJECT_ROOT}"/.github/workflows/*.yaml; do
    fix_file "$f"
done

# 14. Git configuration files
fix_file "${PROJECT_ROOT}/.gitattributes"
fix_file "${PROJECT_ROOT}/.gitignore"

echo ""
echo -e "${GREEN}Done.${NC} CRLF fixed: ${converted}, BOM stripped: ${bom_fixed}, Already OK: ${skipped}"
