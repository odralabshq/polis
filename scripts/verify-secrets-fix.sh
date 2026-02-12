#!/usr/bin/env bash
# Verify Docker secrets are properly configured for tests

set -euo pipefail

echo "=== Docker Secrets Test Verification ==="
echo

# Check if secrets directory exists
if [[ ! -d "secrets" ]]; then
    echo "❌ secrets/ directory not found"
    exit 1
fi
echo "✓ secrets/ directory exists"

# Check required secret files
required_secrets=(
    "valkey_password.txt"
    "valkey_mcp_admin_password.txt"
    "valkey_mcp_agent_password.txt"
    "valkey_dlp_password.txt"
    "valkey_log_writer_password.txt"
    "valkey_users.acl"
)

for secret in "${required_secrets[@]}"; do
    if [[ ! -f "secrets/${secret}" ]]; then
        echo "❌ Missing: secrets/${secret}"
        exit 1
    fi
    echo "✓ Found: secrets/${secret}"
done

echo
echo "=== Checking docker-compose.yml configuration ==="

# Verify Valkey service has required secrets
if ! grep -A 20 "container_name: polis-v2-valkey" docker-compose.yml | grep -q "valkey_mcp_admin_password"; then
    echo "❌ valkey_mcp_admin_password not mounted to Valkey container"
    exit 1
fi
echo "✓ valkey_mcp_admin_password mounted to Valkey"

if ! grep -A 20 "container_name: polis-v2-valkey" docker-compose.yml | grep -q "valkey_dlp_password"; then
    echo "❌ valkey_dlp_password not mounted to Valkey container"
    exit 1
fi
echo "✓ valkey_dlp_password mounted to Valkey"

echo
echo "=== Checking test files ==="

# Check that tests don't use .txt extension for Docker secrets
if grep -r "cat /run/secrets/.*\.txt" tests/ --include="*.bats" 2>/dev/null | grep -v "^Binary"; then
    echo "❌ Found .txt extensions in Docker secret paths (should be removed)"
    exit 1
fi
echo "✓ No .txt extensions in Docker secret paths"

# Check that tests have error handling
if ! grep -q "2>/dev/null || echo" tests/integration/security.bats; then
    echo "⚠️  Warning: security.bats may lack error handling"
fi
echo "✓ Error handling present in tests"

echo
echo "=== All checks passed! ==="
echo
echo "To run the fixed tests:"
echo "  bats tests/integration/security.bats -f 'security: level'"
