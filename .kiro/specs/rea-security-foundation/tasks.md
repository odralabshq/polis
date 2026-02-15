# Implementation Plan: REA Security Foundation

## Overview

This implementation plan covers the Security Foundation (Phase 0) for the Runtime Exception API. Tasks are organized by repository and build incrementally, with property-based tests validating correctness properties from the design.

## Tasks

- [x] 1. Implement Token Module (polis-cli)
  - [x] 1.1 Create token.rs with generate_admin_token function
    - Add `rand`, `base64`, `dirs` dependencies to Cargo.toml
    - Implement 48-byte OsRng token generation with URL-safe base64 encoding
    - _Requirements: 1.1, 1.2, 1.3, 1.4_
  
  - [x] 1.2 Implement token storage functions
    - Implement `write_admin_token` with mode 0600 permissions
    - Implement `read_admin_token` with whitespace trimming
    - Implement `token_exists` helper function
    - Create `~/.polis/session` directory if needed
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_
  
  - [x] 1.3 Write property tests for token module
    - **Property 1: Token Format Validity**
    - **Property 2: Token Storage Round-Trip**
    - **Property 3: Token File Permissions**
    - **Property 4: Whitespace Trimming**
    - **Validates: Requirements 1.1, 1.2, 1.4, 2.1, 2.3, 2.4**

- [x] 2. Integrate Token Lifecycle into CLI Commands (polis-cli)
  - [x] 2.1 Modify init.rs for token generation
    - Add `--regenerate-token` flag to init command
    - Generate token if not exists or if regenerate flag set
    - Log token generation
    - _Requirements: 3.1, 3.2, 3.3, 3.5_
  
  - [x] 2.2 Modify up.rs for token rotation
    - Rotate token before starting containers
    - Log token rotation
    - _Requirements: 3.4, 3.5_
  
  - [x] 2.3 Write property tests for CLI token lifecycle
    - **Property 5: Init Preserves Existing Token**
    - **Property 6: Regenerate Creates New Token**
    - **Property 7: Up Rotates Token**
    - **Validates: Requirements 3.2, 3.3, 3.4**


- [x] 3. Update Docker Compose Security Configuration (polis-cli)
  - [x] 3.1 Harden governance service in docker-compose.yml
    - Mount token as secret file: `${HOME}/.polis/session/admin-token:/run/secrets/admin_token:ro`
    - Bind port to localhost only: `127.0.0.1:8082:8082`
    - Remove any POLIS_ADMIN_TOKEN environment variable if present
    - _Requirements: 4.1, 4.2, 4.3, 4.4_

- [x] 4. Checkpoint - Token Generation Complete
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. Implement Auth Middleware (polis-governance)
  - [x] 5.1 Create auth.rs with middleware function
    - Add `subtle` dependency to Cargo.toml
    - Implement `read_admin_token` from `/run/secrets/admin_token`
    - Implement `validate_token` with constant-time comparison
    - Implement `require_admin_token` middleware
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6_
  
  - [x] 5.2 Wire auth middleware into API router
    - Apply middleware to all routes (GET and POST/DELETE)
    - _Requirements: 5.2_
  
  - [x] 5.3 Write property tests for auth middleware
    - **Property 8: Missing Header Returns 401**
    - **Property 9: Invalid Token Returns 401**
    - **Validates: Requirements 5.2, 5.3, 5.4**

- [x] 6. Implement Rate Limiter (polis-governance)
  - [x] 6.1 Create rate_limiter.rs with WriteRateLimiter
    - Add `dashmap`, `chrono`, `thiserror` dependencies to Cargo.toml
    - Implement `WriteRateLimiter` struct with session and hourly limits
    - Implement `check` method with timestamp eviction
    - Implement `record` method
    - Implement `RateLimitError` enum
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 8.1, 8.2, 8.3, 8.4_
  
  - [x] 6.2 Implement ReadRateLimiter
    - Implement `ReadRateLimiter` struct with minute limit
    - Implement `check` and `record` methods
    - _Requirements: 7.1, 7.2, 7.3_
  
  - [x] 6.3 Create rate limit middleware
    - Create middleware that checks rate limits before processing
    - Return 429 with Retry-After header when exceeded
    - _Requirements: 6.3, 6.4, 7.2_
  
  - [x] 6.4 Write property tests for rate limiter
    - **Property 10: Session Limit Enforced**
    - **Property 11: Hourly Limit Enforced**
    - **Property 12: Retry-After Calculation**
    - **Property 13: Expired Timestamps Evicted**
    - **Property 14: Read Limit Enforced**
    - **Validates: Requirements 6.1, 6.2, 6.5, 6.6, 7.1, 7.3**


- [x] 7. Wire Rate Limiter into API (polis-governance)
  - [x] 7.1 Integrate rate limiters with API routes
    - Apply write rate limiter to POST/DELETE endpoints
    - Apply read rate limiter to GET endpoints
    - _Requirements: 6.1, 6.2, 7.1_

- [x] 8. Checkpoint - Auth and Rate Limiting Complete
  - Ensure all tests pass, ask the user if questions arise.

- [x] 9. Integration Testing
  - [x] 9.1 Write integration tests for auth flow
    - Test authenticated request succeeds
    - Test unauthenticated request returns 401
    - Test invalid token returns 401
    - _Requirements: 5.2, 5.3, 5.4_
  
  - [x] 9.2 Write integration tests for rate limiting
    - Test write rate limit enforcement
    - Test read rate limit enforcement
    - Test Retry-After header presence
    - _Requirements: 6.1, 6.2, 7.1_

- [x] 10. Final Checkpoint
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- All tasks are required for comprehensive implementation
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties
- Unit tests validate specific examples and edge cases
- Split file edits into chunks of maximum 50 lines each
