# Requirements: Value-Based Exceptions for HITL Approval System

## Overview

Extend the polis HITL approval system to support persistent value-based exceptions. Currently, approvals are per-request with a 5-minute TTL. This feature allows users to create persistent exceptions for specific credential values (stored as SHA-256 hashes) going to specific destinations, so the same credential→destination pair is not blocked repeatedly.

## Actors

- **Human Operator** — Creates exceptions via HITL proxy flow or CLI
- **AI Agent** — Triggers blocks, reads exception status (read-only, cannot create exceptions)
- **DLP Module** — Checks exception store before blocking credential matches
- **RESPMOD Module** — Processes `/polis-except` OTT flow to create exceptions

## Requirements

### R1: Credential Hash Computation at Block Time
- R1.1: When the DLP module detects a credential match and blocks the request, it SHALL compute the SHA-256 hash of the matched credential value
- R1.2: The SHA-256 hash SHALL be stored as a 64-character lowercase hex string
- R1.3: The first 4 characters of the raw credential value SHALL be stored as a display prefix (e.g., "sk-a")
- R1.4: The `BlockedRequest` struct SHALL include a new `credential_hash` field (Option<String>) and a `credential_prefix` field (Option<String>)
- R1.5: The credential value itself SHALL NOT be stored anywhere — only the hash and prefix

### R2: Exception Storage in Valkey
- R2.1: Exceptions SHALL be stored with compound key: `polis:exception:value:{sha256_hex_prefix_16}:{host}`
- R2.2: The value SHALL be a JSON object containing: full SHA-256 hash (64 chars), credential prefix (4 chars), destination host, pattern name, creation timestamp, source channel, and TTL
- R2.3: Default TTL SHALL be 30 days (2592000 seconds), configurable via CLI
- R2.4: CLI MAY create permanent exceptions (no TTL) using `--permanent` flag
- R2.5: HITL proxy flow SHALL always use the 30-day default TTL (no permanent exceptions via proxy)
- R2.6: Wildcard destination (`*`) SHALL only be available via CLI, not via HITL proxy flow
- R2.7: A configurable maximum exception count (default: 1000) SHALL be enforced

### R3: DLP Exception Lookup
- R3.1: After a credential pattern match, the DLP module SHALL compute SHA-256 of the matched value and check the exception store before blocking
- R3.2: The lookup SHALL use the 16-char hex prefix + host as the Valkey key
- R3.3: The DLP module SHALL compare the full 64-char SHA-256 from the stored record against the computed hash (not just the key prefix)
- R3.4: If a matching exception exists, the request SHALL be allowed through without blocking
- R3.5: Wildcard destination exceptions (`*`) SHALL match any host
- R3.6: Exception lookup SHALL have a 5ms timeout — on timeout, proceed with normal blocking (fail closed)
- R3.7: The `dlp-reader` Valkey ACL SHALL be updated to include read access to `polis:exception:value:*`

### R4: HITL Proxy Exception Flow (`/polis-except`)
- R4.1: A new command `/polis-except {request_id}` SHALL be recognized by the REQMOD module alongside `/polis-approve`
- R4.2: The REQMOD module SHALL rewrite `/polis-except req-*` with an OTT code, identical to the `/polis-approve` flow
- R4.3: The OTT mapping SHALL include an `action` field distinguishing "approve" from "except"
- R4.4: The RESPMOD module SHALL process exception OTTs by reading the `credential_hash` and `destination` from the `BlockedRequest` record
- R4.5: The RESPMOD module SHALL create the exception atomically using MULTI/EXEC, including: create exception key, write audit log, delete blocked key, delete OTT key
- R4.6: If the `BlockedRequest` lacks a `credential_hash`, the exception creation SHALL fail closed (no exception created)
- R4.7: The exception destination SHALL be the host from the blocked request (no wildcard via proxy)

### R5: CLI Exception Management
- R5.1: `polis-approve exception add {request_id}` SHALL create an exception from a blocked request's credential_hash and destination
- R5.2: `polis-approve exception add --hash {sha256} --dest {host|*}` SHALL create an exception manually
- R5.3: `polis-approve exception add ... --ttl {days}` SHALL set a custom TTL (default: 30 days)
- R5.4: `polis-approve exception add ... --permanent` SHALL create an exception with no TTL
- R5.5: `polis-approve exception list` SHALL display all active exceptions with prefix, destination, TTL remaining, and creation time
- R5.6: `polis-approve exception remove {exception_id}` SHALL delete a specific exception
- R5.7: `polis-approve exception inspect {request_id}` SHALL show the credential hash and prefix for a blocked request
- R5.8: All exception operations SHALL be audit logged to `polis:log:events`

### R6: Valkey ACL Updates
- R6.1: `dlp-reader` SHALL gain read access to `polis:exception:value:*` keys (+GET)
- R6.2: `governance-respmod` SHALL gain write access to `polis:exception:value:*` keys (+SET, +SETEX, +DEL)
- R6.3: `mcp-admin` already has `~polis:*` access — no change needed
- R6.4: `mcp-agent` SHALL NOT have any access to `polis:exception:*` keys

### R7: Rust Type Updates
- R7.1: `BlockedRequest` struct SHALL add `credential_hash: Option<String>` and `credential_prefix: Option<String>` fields
- R7.2: A new `ValueException` struct SHALL be added with fields: hash, prefix, destination, pattern_name, created_at, source, ttl_secs
- R7.3: New Valkey key constants and helpers SHALL be added to `redis_keys.rs`
- R7.4: A new `ExceptionSource` enum SHALL be added: `ProxyInterception`, `Cli`
- R7.5: The `OttMapping` struct SHALL add an `action: OttAction` field with variants `Approve` and `Except`

### R8: Security Invariants
- R8.1: The agent SHALL NOT be able to create, modify, or delete exceptions (enforced by Valkey ACL)
- R8.2: Exception creation via proxy SHALL use the same OTT security model as approvals (time-gate, domain scoping, context binding)
- R8.3: SHA-256 SHALL be used for credential hashing (appropriate for high-entropy API keys)
- R8.4: The DLP module SHALL always compare the full 64-char hash, not just the 16-char key prefix
- R8.5: All exception mutations SHALL be audit logged with source channel and timestamp

### R9: Implementation Constraints
- R9.1: File edits SHALL be split into chunks of maximum 50 lines each
- R9.2: The DLP module's existing behavior SHALL not change for requests without exceptions
- R9.3: The OTT rewrite regex SHALL be extended to match both `/polis-approve` and `/polis-except`
- R9.4: Backward compatibility with existing `BlockedRequest` records (without credential_hash) SHALL be maintained
