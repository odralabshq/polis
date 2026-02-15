# Requirements Document

## Introduction

This document specifies the requirements for the Governance API (Phase 1) of the Runtime Exception API. This phase implements the core data structures and API endpoints for managing blocked requests and domain exceptions. It builds on REA-001 (Security Foundation) which provides authentication middleware and rate limiting infrastructure.

The implementation is entirely within polis-governance and includes:
- RecentBlocksBuffer: Ring buffer for tracking recent blocked requests
- ExceptionStore: Dual-storage exception store with O(1) lookup
- File watcher for hot reload of permanent exceptions
- API endpoints for blocks and exceptions management
- Decision engine integration for exception checking

## Glossary

- **RecentBlocksBuffer**: A thread-safe ring buffer that stores the most recent blocked requests with a capacity of 100 entries and a maximum age of 10 minutes.
- **BlockEvent**: A record of a blocked request containing id, timestamp, block_type, value, reason, and can_exception flag.
- **BlockType**: An enum representing the type of block: Domain, Secret, or Pii.
- **ExceptionStore**: A dual-storage data structure holding session exceptions (in-memory) and permanent exceptions (from file).
- **Exception**: A record allowing a domain to bypass policy rules, containing id, type, value, scope, expires_at, created_at, created_by, and reason.
- **ExceptionType**: An enum representing the type of exception: Domain or Pattern.
- **ExceptionScope**: An enum representing exception duration: Session, Duration { hours }, or Permanent.
- **Domain_Normalization**: The process of converting a domain to canonical form: lowercase, trim trailing dot, remove port.
- **File_Watcher**: A notify-based watcher that monitors ~/.polis/state/exceptions.yaml for changes and triggers reload.
- **Decision_Engine**: The component that evaluates requests and determines allow/block/redact decisions.
- **Auth_Middleware**: The authentication middleware from REA-001 that validates X-Polis-Admin-Token header.

## Requirements

### Requirement 1: Recent Blocks Buffer Structure

**User Story:** As a Polis operator, I want blocked requests stored in a ring buffer, so that I can review recent blocks without unbounded memory growth.

#### Acceptance Criteria

1. THE RecentBlocksBuffer SHALL have a fixed capacity of 100 entries
2. THE RecentBlocksBuffer SHALL use parking_lot::RwLock for thread-safe access
3. WHEN the buffer reaches capacity, THE RecentBlocksBuffer SHALL evict the oldest entry before adding a new one
4. THE RecentBlocksBuffer SHALL store BlockEvent records with id, timestamp, block_type, value, reason, and can_exception fields
5. THE BlockEvent id SHALL use the prefix "blk-" followed by a unique identifier

### Requirement 2: Recent Blocks Buffer Operations

**User Story:** As a Polis operator, I want to query recent blocks with filtering, so that I can find relevant blocked requests.

#### Acceptance Criteria

1. WHEN get_recent is called without a since parameter, THE RecentBlocksBuffer SHALL return blocks from the last 10 minutes
2. WHEN get_recent is called with a since parameter, THE RecentBlocksBuffer SHALL return blocks newer than the specified timestamp
3. THE RecentBlocksBuffer SHALL exclude blocks older than 10 minutes from get_recent results regardless of since parameter
4. WHEN get_by_id is called, THE RecentBlocksBuffer SHALL return the matching BlockEvent or None

### Requirement 3: Exception Store Structure

**User Story:** As a Polis operator, I want exceptions stored with O(1) lookup, so that exception checking does not impact request latency.

#### Acceptance Criteria

1. THE ExceptionStore SHALL maintain two DashMap collections: session_exceptions and permanent_exceptions
2. THE ExceptionStore SHALL use normalized domain as the key for O(1) lookup
3. THE Exception record SHALL contain id, exception_type, value, scope, expires_at, created_at, created_by, and reason fields
4. THE Exception id SHALL use the prefix "exc-" followed by a unique identifier
5. THE ExceptionScope enum SHALL support Session, Duration { hours: u32 }, and Permanent variants

### Requirement 4: Domain Normalization

**User Story:** As a Polis operator, I want domain matching to be case-insensitive and handle common variations, so that exceptions work reliably.

#### Acceptance Criteria

1. THE ExceptionStore SHALL normalize domains by converting to lowercase
2. THE ExceptionStore SHALL normalize domains by trimming trailing dots
3. THE ExceptionStore SHALL normalize domains by removing port numbers
4. WHEN checking is_excepted, THE ExceptionStore SHALL normalize the input domain before lookup

### Requirement 5: Exception Lookup

**User Story:** As a Polis operator, I want exception checking to be fast and handle expiry, so that the decision engine can efficiently check exceptions.

#### Acceptance Criteria

1. THE is_excepted method SHALL check session_exceptions first, then permanent_exceptions
2. THE is_excepted method SHALL return true if a matching non-expired exception exists
3. WHEN an exception has expires_at set, THE is_excepted method SHALL return false if the current time exceeds expires_at
4. WHEN an exception has expires_at as None, THE is_excepted method SHALL treat it as non-expiring
5. THE is_excepted method SHALL complete in O(1) time complexity

### Requirement 6: Exception Management

**User Story:** As a Polis operator, I want to add and remove exceptions, so that I can manage domain allowlisting.

#### Acceptance Criteria

1. WHEN add is called with a new exception, THE ExceptionStore SHALL store it in the appropriate collection based on scope
2. WHEN add is called with a Session scope, THE ExceptionStore SHALL store in session_exceptions
3. WHEN add is called with Duration or Permanent scope, THE ExceptionStore SHALL store in permanent_exceptions
4. WHEN add is called with a duplicate domain, THE ExceptionStore SHALL return an AlreadyExists error with the existing exception id
5. WHEN remove is called with a valid id, THE ExceptionStore SHALL remove the exception from the appropriate collection
6. WHEN remove is called with an invalid id, THE ExceptionStore SHALL return a NotFound error
7. THE get_all method SHALL return all active exceptions from both collections

### Requirement 7: File Watcher

**User Story:** As a Polis operator, I want permanent exceptions to reload automatically when the file changes, so that I can update exceptions without restarting.

#### Acceptance Criteria

1. THE File_Watcher SHALL monitor ~/.polis/state/exceptions.yaml for changes using the notify crate
2. WHEN the exceptions.yaml file is modified or created, THE File_Watcher SHALL trigger a reload within 2 seconds
3. WHEN the exceptions.yaml file contains valid YAML, THE ExceptionStore SHALL update permanent_exceptions with the new content
4. IF the exceptions.yaml file contains invalid YAML, THEN THE ExceptionStore SHALL log a warning and skip the reload
5. THE File_Watcher SHALL only watch for .yaml file changes in the state directory

### Requirement 8: GET /blocks Endpoint

**User Story:** As a Polis operator, I want to retrieve recent blocks via API, so that I can review what requests were blocked.

#### Acceptance Criteria

1. THE GET /blocks endpoint SHALL require X-Polis-Admin-Token header authentication
2. THE GET /blocks endpoint SHALL return blocks from the RecentBlocksBuffer
3. WHEN a since query parameter is provided, THE endpoint SHALL filter blocks to those newer than the timestamp
4. THE response SHALL include blocks array, total count, buffer_size (100), and buffer_age_limit ("10 minutes")
5. WHEN authentication fails, THE endpoint SHALL return HTTP 401 Unauthorized

### Requirement 9: GET /exceptions Endpoint

**User Story:** As a Polis operator, I want to list all active exceptions via API, so that I can review current allowlisting.

#### Acceptance Criteria

1. THE GET /exceptions endpoint SHALL require X-Polis-Admin-Token header authentication
2. THE GET /exceptions endpoint SHALL return all active exceptions from both session and permanent collections
3. THE response SHALL include exceptions array and total count
4. THE endpoint SHALL exclude expired exceptions from the response

### Requirement 10: POST /exceptions/domains Endpoint

**User Story:** As a Polis operator, I want to create domain exceptions via API, so that I can allow blocked domains.

#### Acceptance Criteria

1. THE POST /exceptions/domains endpoint SHALL require X-Polis-Admin-Token header authentication
2. THE request body SHALL accept domain, scope, and optional reason fields
3. WHEN a valid request is received, THE endpoint SHALL create an exception and return HTTP 201 with the exception object
4. WHEN the domain already has an exception, THE endpoint SHALL return HTTP 409 Conflict with existing_id in the response
5. WHEN the domain starts with "*.", THE endpoint SHALL return HTTP 400 Bad Request with "Wildcard not allowed" message
6. WHEN rate limited, THE endpoint SHALL return HTTP 429 Too Many Requests with reset_in_seconds

### Requirement 11: DELETE /exceptions/:id Endpoint

**User Story:** As a Polis operator, I want to remove exceptions via API, so that I can revoke domain allowlisting.

#### Acceptance Criteria

1. THE DELETE /exceptions/:id endpoint SHALL require X-Polis-Admin-Token header authentication
2. WHEN the exception id exists, THE endpoint SHALL remove it and return HTTP 204 No Content
3. WHEN the exception id does not exist, THE endpoint SHALL return HTTP 404 Not Found

### Requirement 12: Decision Engine Integration

**User Story:** As a Polis operator, I want exceptions checked before policy rules, so that allowed domains bypass normal blocking.

#### Acceptance Criteria

1. THE Decision_Engine SHALL check is_excepted BEFORE applying policy rules
2. WHEN a domain is excepted, THE Decision_Engine SHALL return Decision::Allow immediately
3. WHEN a request is blocked, THE Decision_Engine SHALL record a BlockEvent to the RecentBlocksBuffer
4. THE BlockEvent SHALL include the domain, block reason, and can_exception flag

### Requirement 13: Block Recording

**User Story:** As a Polis operator, I want all blocks recorded with sufficient detail, so that I can create exceptions from blocks.

#### Acceptance Criteria

1. WHEN recording a block, THE Decision_Engine SHALL generate a unique id with "blk-" prefix
2. THE BlockEvent SHALL include the current timestamp
3. THE BlockEvent SHALL include the BlockType (Domain, Secret, or Pii)
4. THE BlockEvent SHALL include the blocked value (e.g., domain name)
5. THE BlockEvent SHALL include the reason for blocking
6. THE BlockEvent SHALL set can_exception to true for Domain blocks and false for Secret/Pii blocks


## Implementation Notes
- When implementing file edits, split changes into chunks of maximum 50 lines each to ensure reliable file operations