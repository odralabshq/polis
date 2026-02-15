# Requirements Document

## Introduction

This document specifies the requirements for REA-003: CLI Commands for the Runtime Exception API. This feature implements the `polis manage` subcommand group with `blocked`, `permit`, `revoke`, and `permits` commands in polis-cli that interact with the Governance API (REA-002). These commands enable users to view blocked requests, add domain exceptions (permits), remove exceptions, and list active permits through a command-line interface.

**Design Reference:** RUNTIME-EXCEPTION-API-DESIGN-V2.md (v2.3)

## Command Naming Rationale (v2.3)

| Old Name | New Name | Purpose |
|----------|----------|---------|
| `blocks` | `denials` | View log of recently denied requests (activity) |
| `allow` | `allow` | Add a domain to the allowlist |
| `deny` | `revoke` | Remove a domain from the allowlist |
| `exceptions` | `allowlist` | View the list of active allowed domains |

## Command Grouping (v2.3)

Commands are grouped under `polis manage` to separate lifecycle commands from runtime management:

```
Lifecycle Commands (top-level):
  polis init, up, down, status, logs, doctor, ssh, shell, agents

Management Commands (grouped):
  polis manage denials     # View log of recently denied requests
  polis manage allow       # Add a domain to the allowlist
  polis manage revoke      # Remove a domain from the allowlist
  polis manage allowlist   # View the list of active allowed domains
```

## Glossary

- **CLI**: Command Line Interface - the polis-cli application that runs on the host machine
- **Governance_API**: The HTTP API exposed by polis-governance on port 8082 for managing exceptions
- **Admin_Token**: A 64-character authentication token stored in ~/.polis/session/admin-token
- **Block**: A record of a blocked request stored in the governance service's ring buffer
- **Exception**: A rule that allows a previously blocked domain to bypass policy rules
- **Permit**: User-facing term for adding to the allowlist (v2.3 naming)
- **Session_Exception**: An exception that exists only in memory and is lost on workspace restart
- **Permanent_Exception**: An exception persisted to ~/.polis/state/exceptions.yaml (global)
- **Duration_Exception**: An exception with a time-bound expiration (1h, 2h, 4h, 8h, 12h, 24h)
- **HTTP_Client**: The reqwest-based client for communicating with the Governance API
- **Spinner**: A progress indicator shown during API calls
- **File_Lock**: An exclusive flock() lock used to prevent race conditions during file writes
- **ManageCommands**: The subcommand enum containing Denials, Allow, Revoke, Allowlist variants

## Requirements

### Requirement 1: HTTP Client Infrastructure

**User Story:** As a CLI developer, I want a robust HTTP client for communicating with the Governance API, so that commands can reliably interact with the governance service.

#### Acceptance Criteria

1. THE HTTP_Client SHALL use a 10-second request timeout for all API calls
2. THE HTTP_Client SHALL use a 5-second connection timeout
3. THE HTTP_Client SHALL include the Admin_Token in the X-Polis-Admin-Token header for all requests
4. WHEN a request fails due to timeout or connection error, THE HTTP_Client SHALL retry up to 3 times with exponential backoff (100ms, 200ms, 400ms)
5. THE HTTP_Client SHALL NOT retry on 4xx HTTP status codes
6. THE HTTP_Client SHALL use http://localhost:8082 as the base URL
7. THE HTTP_Client SHALL provide get(), post(), and delete() methods for API operations

### Requirement 2: Duration Parsing

**User Story:** As a user, I want to specify exception durations using human-readable formats, so that I can easily create time-bound exceptions.

#### Acceptance Criteria

1. WHEN a user provides a duration string, THE Duration_Parser SHALL accept the following values: 1h, 2h, 4h, 8h, 12h, 24h
2. WHEN a user provides an invalid duration format (e.g., "3h", "1d", "abc"), THE Duration_Parser SHALL return an error with the message "Invalid duration {value}. Allowed: 1h, 2h, 4h, 8h, 12h, 24h"
3. THE Duration_Parser SHALL be case-insensitive (accept "1H" and "1h")
4. THE Duration_Parser SHALL trim whitespace from input before parsing

### Requirement 3: UI Components

**User Story:** As a user, I want visual feedback during CLI operations, so that I know the command is working and can understand any errors.

#### Acceptance Criteria

1. WHEN an API call is in progress, THE CLI SHALL display a Spinner with a descriptive message
2. WHEN an API call completes successfully, THE Spinner SHALL display a success indicator (✓)
3. WHEN an API call fails, THE Spinner SHALL be cleared before displaying the error
4. THE CLI SHALL NOT use colors when stdout is not a TTY
5. WHEN a 401 error is received, THE CLI SHALL display "Error: Authentication failed" with suggestions: "polis down && polis up" and "polis init --regenerate-token"
6. WHEN a 429 error is received, THE CLI SHALL display "Error: Rate limit exceeded" with the reset time in seconds
7. WHEN a 409 error is received, THE CLI SHALL display "Error: Exception already exists" with the existing exception ID
8. WHEN a connection error occurs, THE CLI SHALL display "Is polis running? Try 'polis status'"

### Requirement 4: polis manage denials Command

**User Story:** As a user, I want to view recently denied requests, so that I can identify domains that need to be added to the allowlist.

#### Acceptance Criteria

1. WHEN a user runs `polis manage denials`, THE CLI SHALL GET /blocks and display results in a table format with columns: ID, TYPE, DOMAIN, REASON, AGO
2. WHEN a user runs `polis manage denials --all`, THE CLI SHALL include all denials in the buffer (not just recent)
3. WHEN a user runs `polis manage denials --json`, THE CLI SHALL output the response as JSON
4. WHEN a user runs `polis manage denials --type <TYPE>`, THE CLI SHALL filter results by denial type (domain, secret, pii)
5. WHEN a user runs `polis manage denials --interactive`, THE CLI SHALL display a selection UI allowing the user to choose a denial to allow
6. WHEN a user selects a denial in interactive mode, THE CLI SHALL add that domain to the allowlist as a session exception

### Requirement 5: polis manage allow Command

**User Story:** As a user, I want to add domains to the allowlist, so that I can allow denied domains to bypass policy rules.

#### Acceptance Criteria

1. WHEN a user runs `polis manage allow <domain>`, THE CLI SHALL POST to /exceptions/domains to create a session exception
2. WHEN a user runs `polis manage allow --block <ID>`, THE CLI SHALL fetch the denial by ID and add its domain to the allowlist
3. WHEN a user runs `polis manage allow --last`, THE CLI SHALL fetch the most recent denial and add its domain to the allowlist
4. IF the denial buffer is empty when using --last, THEN THE CLI SHALL display "No recent denials found"
5. WHEN a user runs `polis manage allow --for <DURATION>`, THE CLI SHALL create a time-bound allowlist entry with the specified duration
6. WHEN a user runs `polis manage allow --for 3h`, THE CLI SHALL display an error with allowed duration values
7. WHEN a user runs `polis manage allow --permanent`, THE CLI SHALL add to the allowlist AND write it to ~/.polis/state/exceptions.yaml
8. WHEN a user runs `polis manage allow --reason <TEXT>`, THE CLI SHALL include the reason in the allowlist entry for audit trail
9. WHEN writing to exceptions.yaml, THE CLI SHALL use flock() exclusive lock to prevent race conditions
10. WHEN writing to exceptions.yaml, THE CLI SHALL use atomic write (temp file + rename)
11. WHEN a domain is added to the allowlist successfully, THE CLI SHALL display the entry details including expiration time if applicable
12. WHEN a session allowlist entry is created, THE CLI SHALL warn that it will be lost on workspace restart

### Requirement 6: polis manage revoke Command

**User Story:** As a user, I want to remove domains from the allowlist, so that I can revoke previously allowed domains.

#### Acceptance Criteria

1. WHEN a user runs `polis manage revoke <domain>`, THE CLI SHALL find the allowlist entry by domain and DELETE /exceptions/:id
2. WHEN a user runs `polis manage revoke --id <ID>`, THE CLI SHALL DELETE /exceptions/:id directly
3. WHEN a user runs `polis manage revoke --all-session`, THE CLI SHALL remove all session allowlist entries
4. WHEN an allowlist entry is removed successfully, THE CLI SHALL display a confirmation message with the entry ID

### Requirement 7: polis manage allowlist Command

**User Story:** As a user, I want to view the active allowlist, so that I can see what domains are currently allowed.

#### Acceptance Criteria

1. WHEN a user runs `polis manage allowlist`, THE CLI SHALL GET /exceptions and display results in a table format with columns: ID, TYPE, DOMAIN, SCOPE, EXPIRES
2. WHEN a user runs `polis manage allowlist --json`, THE CLI SHALL output the response as JSON
3. WHEN a user runs `polis manage allowlist --scope <SCOPE>`, THE CLI SHALL filter results by scope (session, permanent, duration)

### Requirement 8: Command Registration

**User Story:** As a CLI developer, I want the new commands properly integrated into the CLI structure, so that they follow existing patterns and are discoverable.

#### Acceptance Criteria

1. THE CLI SHALL register a Manage variant in the Commands enum with a ManageCommands subcommand
2. THE CLI SHALL define ManageCommands enum with Denials, Allow, Revoke, Allowlist variants
3. THE CLI SHALL define DenialsArgs, AllowArgs, RevokeArgs, and AllowlistArgs structs using clap derive macros
4. THE CLI SHALL provide run_denials(), run_allow(), run_revoke(), and run_allowlist() entry point functions
5. THE CLI SHALL create command files in src/commands/manage/ directory

### Requirement 9: File Locking for Permanent Exceptions

**User Story:** As a user, I want permanent exceptions to be written safely, so that concurrent CLI instances don't corrupt the exceptions file.

#### Acceptance Criteria

1. WHEN writing to exceptions.yaml, THE CLI SHALL acquire an exclusive flock() lock on ~/.polis/state/exceptions.yaml.lock
2. THE File_Lock SHALL be released automatically when the lock file handle is dropped
3. IF the lock cannot be acquired, THE CLI SHALL block until the lock becomes available
4. THE CLI SHALL create the ~/.polis/state/ directory if it does not exist
