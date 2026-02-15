# Tasks: Value-Based Exceptions for HITL Approval System

## Task 1: Update Rust Types (`polis-common/src/types.rs`)
- [x] Add `credential_hash: Option<String>` field to `BlockedRequest`
- [x] Add `credential_prefix: Option<String>` field to `BlockedRequest`
- [x] Add `ExceptionSource` enum with `ProxyInterception` and `Cli` variants
- [x] Add `ValueException` struct with all fields from design
- [x] Add `OttAction` enum with `Approve` and `Except` variants
- [x] Add `action: OttAction` field to `OttMapping` (with `#[serde(default)]` for backward compat)
- [x] Add serde round-trip tests for new types
- [x] Split edits into max 50-line chunks

## Task 2: Update Valkey Key Schema (`polis-common/src/redis_keys.rs`)
- [x] Add `EXCEPTION_VALUE` constant to `keys` module
- [x] Add `EXCEPTION_DEFAULT_SECS` constant to `ttl` module
- [x] Add `EXCEPT_PREFIX` constant to `approval` module
- [x] Add `exception_value_key()` helper function
- [x] Add `exception_value_wildcard_key()` helper function
- [x] Add `validate_credential_hash()` validation function
- [x] Add `exception_command()` helper function
- [x] Add unit tests for new key helpers and validation
- [x] Export new functions from `lib.rs`
- [x] Split edits into max 50-line chunks

## Task 3: Update Valkey ACL (`secrets/valkey_users.acl`)
- [x] Update `dlp-reader` to add `~polis:exception:value:*` read access
- [x] Update `governance-respmod` to add `~polis:exception:value:*` write access and `+set +multi +exec`

## Task 4: Update DLP Module — SHA-256 + Exception Check (`srv_polis_dlp.c`)
- [x] Add `#include <openssl/evp.h>` for SHA-256
- [x] Add `credential_hash` and `credential_prefix` fields to `dlp_req_data_t`
- [x] Add `compute_sha256()` static function using OpenSSL EVP
- [x] Modify `check_patterns()` to compute SHA-256 on credential match and check exception store
- [x] Add exception lookup logic: GET `polis:exception:value:{prefix}:{host}` then GET `polis:exception:value:{prefix}:*`
- [x] Add full 64-char hash comparison against stored record
- [x] Store credential_hash and credential_prefix in per-request data for BlockedRequest inclusion
- [x] Add `X-polis-Credential-Hash` and `X-polis-Credential-Prefix` headers to 403 response
- [x] Split edits into max 50-line chunks

## Task 5: Update REQMOD — OTT Rewrite Extension (`srv_polis_approval_rewrite.c`)
- [x] Extend approve_pattern regex to match both `/polis-approve` and `/polis-except`
- [x] Extract the command type (approve vs except) from regex group 1
- [x] Update capture group references (group 2 = request_id)
- [x] Include `"action":"approve"` or `"action":"except"` in the OTT mapping JSON stored in Valkey
- [x] Update audit log entry to include action field
- [x] Split edits into max 50-line chunks

## Task 6: Update RESPMOD Module — Exception Processing (`srv_polis_approval.c`)
- [x] Add `EXCEPTION_TTL_SECS` constant (2592000 = 30 days)
- [x] Add forward declaration for `process_ott_exception()`
- [x] Add `parsed_action` field to OTT JSON parsing in `process_ott_approval()`
- [x] Parse `action` field from OTT JSON (defaults to "approve" for backward compat)
- [x] Route to `process_ott_exception()` when action="except"
- [x] Implement `process_ott_exception()`: read BlockedRequest, extract credential_hash + destination
- [x] Fail closed if credential_hash is missing from BlockedRequest
- [x] Compute 16-char hex prefix from credential_hash
- [x] Build exception key and ValueException JSON
- [x] Execute MULTI/EXEC: SETEX exception, ZADD audit, DEL blocked, DEL OTT
- [x] Split edits into max 50-line chunks

## Task 7: Extend CLI with Exception Subcommands (`polis-approve` CLI)
- [x] Add `ExceptionCommands` enum with `Add`, `List`, `Remove`, `Inspect` variants
- [x] Add `Exception` variant to `Commands` enum with sub-subcommands
- [x] Implement `exception add {request_id}` — read blocked request, create exception
- [x] Implement `exception add --hash --dest` — manual exception creation
- [x] Implement `--ttl` and `--permanent` flags
- [x] Implement `exception list` — SCAN for `polis:exception:value:*` keys with TTL display
- [x] Implement `exception remove {exception_id}` — DEL key + audit log
- [x] Implement `exception inspect {request_id}` — show credential_hash from blocked request
- [x] Enforce max exception count (1000) before creation
- [x] Audit log all exception operations
- [x] Update `ReportBlockInput` to accept `credential_hash` and `credential_prefix`
- [x] Update `ReportBlockOutput` to include `exception_command` when hash is present
- [x] Split edits into max 50-line chunks

## Task 8: Update Documentation Spec (`odralabs-docs`)
- [x] Create core-features spec `14-value-based-exceptions.md`
- [x] Document the command reference table (HITL + CLI + MCP mapping)
- [x] Document the Valkey key schema additions
- [x] Document the DLP module integration flow
- [x] Document the REQMOD OTT extension
- [x] Document the RESPMOD exception flow
- [x] Document the Valkey ACL changes
- [x] Document the security invariants
- [x] Document backward compatibility notes
