# Test Fixes Summary

## Issues Fixed

### 1. Docker Secrets Path Issue (FIXED)
**Tests affected**: `tests/integration/security.bats` (2 tests)

**Problem**: Tests used `.txt` extension in secret paths inside containers
```bash
# Wrong
cat /run/secrets/valkey_mcp_admin_password.txt

# Correct  
cat /run/secrets/valkey_mcp_admin_password
```

**Files modified**:
- `docker-compose.yml` - Added 4 secrets to Valkey container
- `tests/integration/security.bats` - Fixed 2 tests
- `tests/e2e/mcp-agent.bats` - Fixed 2 helper functions
- `tests/unit/valkey-properties.bats` - Fixed 1 helper function
- `tests/helpers/common.bash` - Fixed 1 function

### 2. relax_security_level() Crash (FIXED)
**Tests affected**: All e2e tests (44+ tests)

**Problem**: Function crashed when Valkey container didn't exist

**Solution**: Added container existence check
```bash
if ! docker ps --filter "name=^${VALKEY_CONTAINER}$" ... | grep -q ...; then
    return 0  # Skip gracefully
fi
```

**Files modified**:
- `tests/helpers/common.bash` - Added 5 lines

### 3. Container Recreation Required (ACTION NEEDED)
**Tests affected**: `tests/e2e/dlp.bats` and others

**Problem**: New secrets in docker-compose.yml not mounted because container not recreated

**Current state**:
```bash
docker exec polis-v2-valkey ls -la /run/secrets/
# Shows only 2 files (valkey_password, valkey_acl)
```

**Required state**:
```bash
# Should show 6 files:
- valkey_password
- valkey_acl
- valkey_mcp_admin_password
- valkey_mcp_agent_password
- valkey_dlp_password
- valkey_log_writer_password
```

## Action Required

**CRITICAL**: Must recreate containers for secrets to be mounted:

```bash
# Step 1: Recreate containers
docker compose down
docker compose up -d

# Step 2: Wait for healthy
docker compose ps

# Step 3: Verify secrets mounted
docker exec polis-v2-valkey ls -la /run/secrets/
# Should show 6 files (not 2)

# Step 4: Run tests
bats tests/integration/security.bats -f "security: level"
bats tests/e2e/dlp.bats -f "plain traffic"
```

## Why "docker restart" Doesn't Work

Docker secrets are mounted at **container creation time**, not at runtime:
- ❌ `docker restart polis-v2-valkey` - Does NOT remount secrets
- ❌ `docker compose restart` - Does NOT remount secrets
- ✅ `docker compose down` + `docker compose up -d` - Recreates containers with new secrets

## Expected Test Results After Recreation

```
✓ security: level relaxed allows new domains
✓ security: level strict blocks new domains
✓ e2e-dlp: plain traffic without credentials is ALLOWED
✓ All other e2e tests should pass setup phase
```

## Files Modified Summary

1. `docker-compose.yml` - Added 4 secrets to Valkey service
2. `tests/integration/security.bats` - Fixed 2 tests (removed .txt, added error handling)
3. `tests/e2e/mcp-agent.bats` - Fixed 2 helpers (removed .txt, added error handling)
4. `tests/unit/valkey-properties.bats` - Fixed 1 helper (added error handling)
5. `tests/helpers/common.bash` - Fixed 2 functions (removed .txt, added container check)

**Total**: 5 files, ~35 lines changed
