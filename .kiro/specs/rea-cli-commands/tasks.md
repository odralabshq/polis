# Implementation Plan: REA-003 CLI Commands

## Overview

This implementation plan breaks down the CLI commands feature into discrete coding tasks. The implementation follows the existing polis-cli patterns and builds incrementally, ensuring each step is testable before moving to the next.

**Design Reference:** RUNTIME-EXCEPTION-API-DESIGN-V2.md (v2.3)

Commands are grouped under `polis manage` subcommand with new naming:
- `polis manage denials` (was `polis blocks`)
- `polis manage allow` (was `polis allow`)
- `polis manage revoke` (was `polis deny`)
- `polis manage allowlist` (was `polis exceptions`)

## Tasks

- [x] 1. Add dependencies and create module structure
  - Add reqwest, dialoguer, colored, fs2, serde_yaml to Cargo.toml
  - Create src/http_client.rs module
  - Create src/duration.rs module
  - Create src/ui/spinner.rs module
  - Create src/ui/errors.rs module
  - Create src/commands/manage/mod.rs module
  - Create src/commands/manage/denials.rs module
  - Create src/commands/manage/allow.rs module
  - Create src/commands/manage/revoke.rs module
  - Create src/commands/manage/allowlist.rs module
  - Update src/ui/mod.rs to export new modules
  - _Requirements: 1.1, 1.2, 1.6, 1.7, 8.5_

- [x] 2. Implement duration parser
  - [x] 2.1 Implement parse_duration() function
    - Parse duration strings (1h, 2h, 4h, 8h, 12h, 24h)
    - Handle case-insensitivity and whitespace trimming
    - Return DurationParseError for invalid inputs
    - _Requirements: 2.1, 2.2, 2.3, 2.4_
  
  - [x] 2.2 Write property tests for duration parser
    - **Property 4: Invalid Duration Rejection**
    - **Property 5: Duration Case Insensitivity**
    - **Property 6: Duration Whitespace Handling**
    - **Validates: Requirements 2.2, 2.3, 2.4**

- [x] 3. Implement HTTP client
  - [x] 3.1 Implement GovernanceClient struct
    - Create client with 10-second request timeout, 5-second connect timeout
    - Read admin token from ~/.polis/session/admin-token
    - Implement get(), post(), delete() methods
    - _Requirements: 1.1, 1.2, 1.3, 1.6, 1.7_
  
  - [x] 3.2 Implement retry logic with exponential backoff
    - Retry up to 3 times on timeout/connection errors
    - Use delays: 100ms, 200ms, 400ms
    - Do NOT retry on 4xx errors
    - _Requirements: 1.4, 1.5_
  
  - [x] 3.3 Write property tests for HTTP client
    - **Property 1: Auth Token Inclusion**
    - **Property 2: Retry on Transient Errors**
    - **Property 3: No Retry on Client Errors**
    - **Validates: Requirements 1.3, 1.4, 1.5**

- [x] 4. Checkpoint - Ensure duration parser and HTTP client tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. Implement UI components
  - [x] 5.1 Implement Spinner struct
    - Create spinner with message using indicatif
    - Implement set_message(), finish_success(), finish_error(), finish_clear()
    - _Requirements: 3.1, 3.2, 3.3_
  
  - [x] 5.2 Implement error formatting functions
    - Implement format_api_error() for 401, 429, 409, and other errors
    - Implement is_tty() for color support detection
    - Include actionable suggestions in error messages
    - _Requirements: 3.4, 3.5, 3.6, 3.7, 3.8_
  
  - [x] 5.3 Write property tests for error formatting
    - **Property 7: Rate Limit Error Formatting**
    - **Property 8: Conflict Error Formatting**
    - **Validates: Requirements 3.6, 3.7**

- [x] 6. Implement data models
  - [x] 6.1 Create API request/response types
    - Define Block, BlocksResponse structs
    - Define Exception, ExceptionScope, ExceptionsResponse structs
    - Define AddExceptionRequest struct
    - Define ExceptionsFile struct for YAML format
    - _Requirements: 4.1, 5.1, 6.1, 7.1_

- [x] 7. Implement polis manage denials command
  - [x] 7.1 Add DenialsArgs struct and register command
    - Define --all, --json, --type, --interactive flags
    - Add ManageCommands enum with Denials variant
    - Register Manage variant in Commands enum
    - _Requirements: 8.1, 8.2, 8.3_
  
  - [x] 7.2 Implement run_denials() function
    - GET /blocks endpoint
    - Display results in table format
    - Support --json output
    - Support --type filtering
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 8.4_
  
  - [x] 7.3 Implement interactive mode for denials
    - Use dialoguer for selection UI
    - Add selected domain to allowlist as session entry
    - _Requirements: 4.5, 4.6_
  
  - [x] 7.4 Write property test for JSON output
    - **Property 9: JSON Output Validity** (denials part)
    - **Validates: Requirements 4.3**

- [x] 8. Checkpoint - Ensure denials command works
  - Ensure all tests pass, ask the user if questions arise.

- [x] 9. Implement file locking for permanent allowlist entries
  - [x] 9.1 Implement ExceptionFileLock struct
    - Acquire exclusive flock() lock on exceptions.yaml.lock
    - Release lock automatically on drop (RAII pattern)
    - _Requirements: 9.1, 9.2, 9.3_
  
  - [x] 9.2 Implement write_permanent_exception() function in allow.rs
    - Create .polis/state/ directory if needed
    - Load existing exceptions.yaml or create new
    - Write atomically using temp file + rename
    - _Requirements: 5.7, 5.9, 5.10, 9.4_

- [x] 10. Implement polis manage allow command
  - [x] 10.1 Add AllowArgs struct and register command
    - Define domain arg, --block, --last, --for, --permanent, --reason flags
    - Add Allow variant to ManageCommands enum
    - _Requirements: 8.2, 8.3_
  
  - [x] 10.2 Implement run_allow() function
    - Resolve domain from args (direct, --block, or --last)
    - POST to /exceptions/domains
    - Handle --for duration parsing
    - Handle --permanent with file write
    - Display success message with allowlist entry details
    - _Requirements: 5.1, 5.2, 5.3, 5.5, 5.7, 5.8, 5.11, 5.12, 8.4_
  
  - [x] 10.3 Write property tests for allow command
    - **Property 10: Reason Preservation**
    - **Property 11: Exception Details Display**
    - **Validates: Requirements 5.8, 5.11**

- [x] 11. Implement polis manage revoke command
  - [x] 11.1 Add RevokeArgs struct and register command
    - Define domain arg, --id, --all-session flags
    - Add Revoke variant to ManageCommands enum
    - _Requirements: 8.2, 8.3_
  
  - [x] 11.2 Implement run_revoke() function
    - Find allowlist entry by domain or use --id directly
    - DELETE /exceptions/:id
    - Handle --all-session for bulk deletion
    - Display confirmation message
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 8.4_
  
  - [x] 11.3 Write property test for revoke command
    - **Property 12: Deletion Confirmation**
    - **Validates: Requirements 6.4**

- [x] 12. Implement polis manage allowlist command
  - [x] 12.1 Add AllowlistArgs struct and register command
    - Define --json, --scope flags
    - Add Allowlist variant to ManageCommands enum
    - _Requirements: 8.2, 8.3_
  
  - [x] 12.2 Implement run_allowlist() function
    - GET /exceptions endpoint
    - Display results in table format
    - Support --json output
    - Support --scope filtering
    - _Requirements: 7.1, 7.2, 7.3, 8.4_
  
  - [x] 12.3 Write property test for JSON output
    - **Property 9: JSON Output Validity** (allowlist part)
    - **Validates: Requirements 7.2**

- [x] 13. Checkpoint - Ensure all commands work
  - Ensure all tests pass, ask the user if questions arise.

- [x] 14. Add error variants to PolisError
  - [x] 14.1 Add new error variants
    - Add HttpError, ApiError, DurationParseError, AdminTokenNotFound, FileLockError, UserError
    - Implement fix_hint() for new variants
    - _Requirements: 3.5, 3.6, 3.7, 3.8_

- [x] 15. Wire commands in main.rs
  - [x] 15.1 Update main.rs to handle new commands
    - Add match arm for Manage variant
    - Match on ManageCommands to call respective run_* functions
    - _Requirements: 8.1, 8.4_

- [x] 16. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 17. Write integration tests
  - [x] 17.1 Write integration test for full workflow
    - Mock governance API with wiremock
    - Test: denials → allow --last → allowlist → revoke flow
    - Test error handling for 401, 429, 409 responses
    - _Requirements: All_

## Notes

- All tasks are required for comprehensive implementation with full test coverage
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties
- Unit tests validate specific examples and edge cases
- The implementation follows existing polis-cli patterns (clap derive, PolisError, ui module)
- Commands are grouped under `polis manage` subcommand (v2.3 design)
- Command files are in src/commands/manage/ directory
