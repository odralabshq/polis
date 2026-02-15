# Implementation Plan: REA Governance API

## Overview

This implementation plan covers Phase 1 of the Runtime Exception API - the Governance API components. All implementation is in polis-governance using Rust. The plan builds on REA-001 (Security Foundation) which provides authentication middleware and rate limiting.

## Tasks

- [x] 1. Add dependencies and create module structure
  - Add `parking_lot = "0.12"` and `dashmap = "5.5"` to Cargo.toml if not present
  - Create src/blocks_buffer.rs
  - Create src/exception_store.rs
  - Create src/exception_file_watcher.rs
  - Create src/api/mod.rs
  - Create src/api/blocks.rs
  - Create src/api/exceptions.rs
  - Update src/lib.rs to export new modules
  - _Requirements: 1.1, 3.1_

- [x] 2. Implement RecentBlocksBuffer
  - [x] 2.1 Implement BlockEvent and BlockType structs
    - Define BlockEvent with id, timestamp, block_type, value, reason, can_exception
    - Define BlockType enum with Domain, Secret, Pii variants
    - Implement Serialize for JSON responses
    - Add generate_block_id() function with "blk-" prefix
    - _Requirements: 1.4, 1.5, 13.3_
  
  - [x] 2.2 Implement RecentBlocksBuffer core
    - Create struct with RwLock<VecDeque<BlockEvent>>, capacity, max_age
    - Implement new() with default capacity 100 and max_age 10 minutes
    - Implement with_config() for custom settings
    - Implement add() with FIFO eviction when at capacity
    - _Requirements: 1.1, 1.2, 1.3_
  
  - [x] 2.3 Implement RecentBlocksBuffer queries
    - Implement get_recent() with optional since parameter
    - Filter by max_age (10 minutes) regardless of since
    - Implement get_by_id() for single block lookup
    - Implement capacity() and max_age() getters
    - _Requirements: 2.1, 2.2, 2.3, 2.4_
  
  - [x] 2.4 Write property tests for RecentBlocksBuffer
    - **Property 1: Buffer Capacity and FIFO Eviction**
    - **Property 2: Block ID Uniqueness and Prefix**
    - **Property 12: get_recent Filtering by Time**
    - **Property 13: get_by_id Correctness**
    - **Validates: Requirements 1.1, 1.3, 1.5, 2.1, 2.2, 2.3, 2.4**

- [x] 3. Implement ExceptionStore
  - [x] 3.1 Implement Exception and related types
    - Define Exception struct with all fields
    - Define ExceptionType enum (Domain, Pattern)
    - Define ExceptionScope enum (Session, Duration, Permanent)
    - Define ExceptionError enum with thiserror
    - Add generate_exception_id() function with "exc-" prefix
    - _Requirements: 3.3, 3.4, 3.5_
  
  - [x] 3.2 Implement ExceptionStore core
    - Create struct with two DashMaps and state_file_path
    - Implement new() constructor
    - Implement normalize_domain() (lowercase, trim dot, remove port)
    - _Requirements: 3.1, 3.2, 4.1, 4.2, 4.3_
  
  - [x] 3.3 Implement is_excepted with O(1) lookup
    - Normalize input domain before lookup
    - Check session_exceptions first, then permanent_exceptions
    - Handle expiry checking for expires_at
    - Return false for expired exceptions
    - _Requirements: 4.4, 5.1, 5.2, 5.3, 5.4, 5.5_
  
  - [x] 3.4 Implement add/remove/get operations
    - Implement add() with duplicate detection
    - Route to session_exceptions or permanent_exceptions based on scope
    - Implement remove() with NotFound error for invalid IDs
    - Implement get_all() returning all active exceptions
    - Implement get_by_id() for single exception lookup
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 6.7_
  
  - [x] 3.5 Write property tests for ExceptionStore
    - **Property 3: Exception ID Uniqueness and Prefix**
    - **Property 4: Domain Normalization Round-Trip**
    - **Property 5: Exception Expiry Handling**
    - **Property 6: Add/Remove Exception Round-Trip**
    - **Property 7: Duplicate Domain Detection**
    - **Validates: Requirements 3.4, 4.1-4.4, 5.2-5.4, 6.1, 6.4, 6.5, 6.7**

- [x] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. Implement File Watcher
  - [x] 5.1 Implement exceptions.yaml file format
    - Define ExceptionsFile struct with version, last_updated, exceptions
    - Implement Serialize/Deserialize for YAML
    - _Requirements: 7.3_
  
  - [x] 5.2 Implement load_permanent and reload_permanent
    - Load exceptions from .polis/state/exceptions.yaml
    - Parse YAML and populate permanent_exceptions DashMap
    - Handle missing file gracefully (empty state)
    - Handle invalid YAML by logging warning and skipping reload
    - _Requirements: 7.3, 7.4_
  
  - [x] 5.3 Implement ExceptionFileWatcher
    - Create watcher using notify crate
    - Watch state directory for .yaml file changes
    - Use mpsc channel to communicate changes
    - Implement next_change() async method
    - _Requirements: 7.1, 7.2, 7.5_
  
  - [x] 5.4 Implement spawn_file_watcher background task
    - Spawn tokio task that watches for changes
    - Call reload_permanent on file change
    - Log reload success/failure
    - _Requirements: 7.2_
  
  - [x] 5.5 Write property tests for file loading
    - **Property 14: YAML Loading Round-Trip**
    - **Validates: Requirements 7.3**

- [x] 6. Implement API Endpoints
  - [x] 6.1 Create ExceptionAppState and router
    - Define ExceptionAppState with blocks_buffer and exception_store
    - Create create_exception_router() function
    - Apply auth middleware from REA-001 to all routes
    - _Requirements: 8.1, 9.1, 10.1, 11.1_
  
  - [x] 6.2 Implement GET /blocks endpoint
    - Parse optional since query parameter
    - Call blocks_buffer.get_recent()
    - Return BlocksResponse with blocks, total, buffer_size, buffer_age_limit
    - _Requirements: 8.2, 8.3, 8.4_
  
  - [x] 6.3 Implement GET /exceptions endpoint
    - Call exception_store.get_all()
    - Filter out expired exceptions
    - Return ExceptionsListResponse with exceptions and total
    - _Requirements: 9.2, 9.3, 9.4_
  
  - [x] 6.4 Implement POST /exceptions/domains endpoint
    - Parse AddExceptionRequest from body
    - Validate domain (reject wildcards starting with "*.")
    - Create Exception with generated ID and timestamps
    - Call exception_store.add()
    - Return 201 with exception or appropriate error response
    - _Requirements: 10.2, 10.3, 10.4, 10.5, 10.6_
  
  - [x] 6.5 Implement DELETE /exceptions/:id endpoint
    - Extract id from path
    - Call exception_store.remove()
    - Return 204 on success, 404 on NotFound
    - _Requirements: 11.2, 11.3_
  
  - [x] 6.6 Write property tests for API endpoints
    - **Property 8: API Authentication Requirement**
    - **Property 9: Wildcard Domain Rejection**
    - **Validates: Requirements 8.1, 9.1, 10.1, 10.5, 11.1**

- [x] 7. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 8. Integrate with Decision Engine
  - [x] 8.1 Add ExceptionStore and BlocksBuffer to DecisionEngine
    - Add exception_store: Arc<ExceptionStore> field
    - Add blocks_buffer: Arc<RecentBlocksBuffer> field
    - Update constructor to accept these dependencies
    - _Requirements: 12.1_
  
  - [x] 8.2 Implement exception checking in evaluate
    - Extract domain from request
    - Call is_excepted() BEFORE applying policy rules
    - Return Decision::Allow immediately if excepted
    - Log exception bypass with tracing
    - _Requirements: 12.1, 12.2_
  
  - [x] 8.3 Implement block recording
    - After policy rules return Block decision
    - Create BlockEvent with all required fields
    - Set can_exception based on BlockType
    - Call blocks_buffer.add()
    - _Requirements: 12.3, 12.4, 13.1, 13.2, 13.3, 13.4, 13.5, 13.6_
  
  - [x] 8.4 Write property tests for decision engine integration
    - **Property 10: Exception Allows Policy Bypass**
    - **Property 11: Block Recording Completeness**
    - **Validates: Requirements 12.1, 12.2, 12.3, 12.4, 13.1-13.6**

- [x] 9. Integrate API with HTTP Server
  - [x] 9.1 Update http.rs to mount exception routes
    - Add exception_store parameter to run_with_config_service
    - Create ExceptionAppState with shared stores
    - Merge exception router with existing router
    - _Requirements: 8.1, 9.1, 10.1, 11.1_
  
  - [x] 9.2 Update main.rs to initialize stores
    - Create RecentBlocksBuffer instance
    - Create ExceptionStore instance with state file path
    - Load permanent exceptions on startup
    - Spawn file watcher background task
    - Pass stores to HTTP server and decision engine
    - _Requirements: 7.2, 7.3_

- [x] 10. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 11. Write integration tests
  - [x] 11.1 Block → Exception → Allow flow test
    - Block a domain via decision engine
    - Verify block appears in GET /blocks
    - Create exception via POST /exceptions/domains
    - Verify subsequent requests are allowed
    - _Requirements: 12.1, 12.2, 12.3_
  
  - [x] 11.2 File watcher reload test
    - Start with empty exceptions
    - Write exceptions.yaml file
    - Verify exceptions loaded within 2 seconds
    - _Requirements: 7.2, 7.3_

## Notes

- All tasks are required for comprehensive implementation
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties
- Unit tests validate specific examples and edge cases
- All API endpoints require authentication from REA-001

