# Requirements Document

## Introduction

This document specifies the requirements for the Security Foundation (Phase 0) of the Runtime Exception API. This phase establishes the authentication, authorization, and rate limiting infrastructure that all subsequent REA phases depend on. The implementation spans two repositories: polis-cli (token generation/rotation) and polis-governance (auth middleware, rate limiting).

## Glossary

- **Admin_Token**: A 48-byte (384 bits entropy) cryptographically secure token used to authenticate requests to the Exception API, encoded as URL-safe base64.
- **OsRng**: Operating system random number generator, cryptographically secure source of randomness from the `rand` crate.
- **Token_File**: The file at `~/.polis/session/admin-token` storing the admin token with mode 0600.
- **Secret_Mount**: Docker volume mount at `/run/secrets/admin_token` providing the token to the governance container.
- **Auth_Middleware**: Axum middleware that validates the `X-Polis-Admin-Token` header on all requests.
- **Rate_Limiter**: Timestamp-based sliding window rate limiter that tracks request counts per session and per hour.
- **Session_Limit**: Maximum number of write operations allowed per container session (20).
- **Hourly_Limit**: Maximum number of write operations allowed per sliding hour window (50).
- **Read_Limit**: Maximum number of read operations allowed per minute (100).
- **Constant_Time_Comparison**: Token comparison using `subtle::ConstantTimeEq` to prevent timing attacks.
- **Polis_CLI**: The command-line interface for managing Polis infrastructure.
- **Polis_Governance**: The governance service that hosts the Exception API.

## Requirements

### Requirement 1: Token Generation

**User Story:** As a Polis operator, I want secure admin tokens generated automatically, so that I can authenticate to the Exception API without manual token management.

#### Acceptance Criteria

1. THE Token_Generator SHALL generate Admin_Token using exactly 48 bytes from OsRng
2. THE Token_Generator SHALL encode the 48 bytes as URL-safe base64 without padding
3. THE Token_Generator SHALL NOT use thread_rng() or any non-cryptographic random source
4. WHEN generating a token, THE Token_Generator SHALL produce a 64-character string

### Requirement 2: Token Storage

**User Story:** As a Polis operator, I want tokens stored securely on disk, so that only authorized users can access them.

#### Acceptance Criteria

1. THE Token_Manager SHALL store Admin_Token at `~/.polis/session/admin-token`
2. THE Token_Manager SHALL create the `~/.polis/session` directory if it does not exist
3. WHEN writing the Token_File, THE Token_Manager SHALL set file permissions to mode 0600
4. THE Token_Manager SHALL trim whitespace when reading the Token_File
5. IF the home directory cannot be determined, THEN THE Token_Manager SHALL return a NoHomeDir error

### Requirement 3: CLI Token Lifecycle

**User Story:** As a Polis operator, I want to control when tokens are created and rotated, so that I can manage security according to my operational needs.

#### Acceptance Criteria

1. WHEN `polis init` runs AND no Token_File exists, THE Polis_CLI SHALL generate and store a new Admin_Token
2. WHEN `polis init` runs AND Token_File exists, THE Polis_CLI SHALL skip token generation
3. WHEN `polis init --regenerate-token` runs, THE Polis_CLI SHALL generate and store a new Admin_Token regardless of existing token
4. WHEN `polis up` runs, THE Polis_CLI SHALL rotate the Admin_Token before starting containers
5. WHEN token rotation occurs, THE Polis_CLI SHALL log that a new token was generated

### Requirement 4: Docker Security Hardening

**User Story:** As a security engineer, I want the governance container hardened against common attack vectors, so that the Exception API is protected from unauthorized access.

#### Acceptance Criteria

1. THE Docker_Compose SHALL mount Token_File as Secret_Mount at `/run/secrets/admin_token` with read-only access
2. THE Docker_Compose SHALL bind the governance port to `127.0.0.1:8082` only
3. THE Docker_Compose SHALL NOT expose the Admin_Token via environment variables
4. THE Docker_Compose SHALL NOT bind the governance port to `0.0.0.0`

### Requirement 5: Authentication Middleware

**User Story:** As a security engineer, I want all API requests authenticated, so that only authorized clients can access the Exception API.

#### Acceptance Criteria

1. THE Auth_Middleware SHALL read the expected token from Secret_Mount at `/run/secrets/admin_token`
2. THE Auth_Middleware SHALL require the `X-Polis-Admin-Token` header on ALL endpoints including GET requests
3. WHEN a request lacks the `X-Polis-Admin-Token` header, THE Auth_Middleware SHALL return HTTP 401 Unauthorized
4. WHEN a request provides an invalid token, THE Auth_Middleware SHALL return HTTP 401 Unauthorized
5. THE Auth_Middleware SHALL use Constant_Time_Comparison for token validation
6. IF the Secret_Mount file is missing or unreadable, THEN THE Auth_Middleware SHALL return HTTP 500 Internal Server Error

### Requirement 6: Write Rate Limiting

**User Story:** As a system administrator, I want write operations rate limited, so that the system is protected from abuse and resource exhaustion.

#### Acceptance Criteria

1. THE Rate_Limiter SHALL enforce a Session_Limit of 20 write operations per container session
2. THE Rate_Limiter SHALL enforce an Hourly_Limit of 50 write operations per sliding hour window
3. WHEN Session_Limit is exceeded, THE Rate_Limiter SHALL return HTTP 429 Too Many Requests
4. WHEN Hourly_Limit is exceeded, THE Rate_Limiter SHALL return HTTP 429 Too Many Requests with Retry-After header
5. THE Rate_Limiter SHALL calculate Retry-After as seconds until the oldest timestamp expires from the hourly window
6. THE Rate_Limiter SHALL use timestamp-based tracking that survives conceptual understanding of time passage

### Requirement 7: Read Rate Limiting

**User Story:** As a system administrator, I want read operations rate limited, so that the system remains responsive under high query load.

#### Acceptance Criteria

1. THE Rate_Limiter SHALL enforce a Read_Limit of 100 read operations per minute
2. WHEN Read_Limit is exceeded, THE Rate_Limiter SHALL return HTTP 429 Too Many Requests with Retry-After header
3. THE Rate_Limiter SHALL evict expired timestamps from the sliding window before checking limits

### Requirement 8: Rate Limiter State Management

**User Story:** As a system administrator, I want rate limiting to be accurate across time, so that limits are enforced correctly regardless of request patterns.

#### Acceptance Criteria

1. THE Rate_Limiter SHALL store timestamps for each recorded operation
2. THE Rate_Limiter SHALL evict timestamps older than the window period before checking limits
3. THE Rate_Limiter SHALL use atomic operations for thread-safe counter updates
4. THE Rate_Limiter SHALL use DashMap for concurrent timestamp storage

## Implementation Notes


