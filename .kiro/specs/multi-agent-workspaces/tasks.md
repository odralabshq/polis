# Implementation Plan: Multi-Agent Workspaces

## Overview

This implementation plan follows the 3-phase approach from the research, building the multi-agent workspace feature incrementally. Each phase delivers testable functionality that builds on the previous phase.

- **Phase 1 (Weeks 1-2)**: Foundation - WorkspaceManager, git worktree creation, granular locking
- **Phase 2 (Weeks 3-4)**: Isolation - Network/Mount/PID namespaces, port manager, cgroup management
- **Phase 3 (Weeks 5-6)**: TUI & Orchestration - Multi-agent view, broadcast mode, CLI polish

## Tasks

### Phase 1: Foundation

- [ ] 1. Set up multi-agent infrastructure in polis-workspace
  - [ ] 1.1 Create `crates/polis-agents/` crate with Cargo.toml
    - Add dependencies: tokio, serde, thiserror, tracing
    - Define crate structure with lib.rs exposing public API
    - _Requirements: 14.1, 14.2_

  - [ ] 1.2 Implement WorkspaceManager core structure
    - Create `WorkspaceManager` struct with paths and agent HashMap
    - Implement `initialize()` to create bare-repo.git and worktrees directory
    - Implement `save_state()` and `load_state()` for state.json persistence
    - _Requirements: 14.1, 14.2, 15.1, 15.2_

  - [ ]* 1.3 Write property test for initialization idempotence
    - **Property 20: Initialization Idempotence**
    - **Validates: Requirements 14.1, 14.2, 14.3, 14.4**

  - [ ] 1.4 Implement state reconciliation on startup
    - Scan worktrees directory for existing worktrees
    - Reconcile against state.json, using filesystem as source of truth
    - Log warnings for orphaned worktrees or missing state entries
    - _Requirements: 15.2, 15.4_

  - [ ]* 1.5 Write property test for persistence round-trip
    - **Property 21: Persistence Round-Trip**
    - **Validates: Requirements 15.1, 15.2, 15.3, 15.5**

- [ ] 2. Implement Git Worktree Management
  - [ ] 2.1 Implement `create_agent()` with git worktree creation
    - Execute `git worktree add` to create worktree at specified path
    - Create branch `agent/<name>` if no branch specified
    - Update state.json with new agent entry
    - _Requirements: 1.1, 1.2, 1.8_

  - [ ]* 2.2 Write property test for agent creation completeness
    - **Property 1: Agent Creation Completeness**
    - **Validates: Requirements 1.1, 1.6, 1.8**

  - [ ] 2.3 Implement `remove_agent()` with worktree cleanup
    - Execute `git worktree remove` to delete worktree
    - Remove agent from state.json
    - _Requirements: 4.2_

  - [ ] 2.4 Implement agent name validation and uniqueness check
    - Validate name format (alphanumeric + hyphens)
    - Check for existing agent with same name
    - Return appropriate error codes
    - _Requirements: 1.3_

  - [ ]* 2.5 Write property test for agent name uniqueness
    - **Property 2: Agent Name Uniqueness**
    - **Validates: Requirements 1.3**

  - [ ] 2.6 Implement `list_agents()` returning agent status
    - Return all agents with name, status, branch, port info
    - _Requirements: 2.1, 2.2_

  - [ ]* 2.7 Write property test for agent listing completeness
    - **Property 3: Agent Listing Completeness**
    - **Validates: Requirements 2.1, 2.2**

- [ ] 3. Checkpoint - Verify worktree management
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 4. Implement Granular Git Mutex
  - [ ] 4.1 Create GitMutex with RwLock pattern
    - Implement `LockLevel` enum (None, RefRead, RefWrite, Exclusive)
    - Implement `lock_level()` to categorize operations
    - Create per-ref lock HashMap for branch operations
    - _Requirements: 10.1, 10.5_

  - [ ] 4.2 Implement lock acquisition with timeouts
    - 5s timeout for branch-level locks
    - 30s timeout for exclusive locks (fetch, gc)
    - Return `LockError::Timeout` on expiration
    - _Requirements: 10.3_

  - [ ]* 4.3 Write property test for granular locking
    - **Property 11: Git Mutex Granular Locking**
    - **Validates: Requirements 10.1, 10.4**

  - [ ]* 4.4 Write property test for lock timeout
    - **Property 12: Git Mutex Timeout**
    - **Validates: Requirements 10.3**

  - [ ]* 4.5 Write property test for non-blocking operations
    - **Property 13: Git Mutex Non-Blocking Operations**
    - **Validates: Requirements 10.5**

- [ ] 5. Implement Tool-Enforced Advisory Locks
  - [ ] 5.1 Create AdvisoryLockManager with file lock tracking
    - Implement `acquire()` and `release()` methods
    - Track lock holder, timestamp, and lock type
    - _Requirements: 6.1, 6.3_

  - [ ] 5.2 Implement ToolFileWriter wrapper
    - Create wrapper that ALL file tools must use
    - Automatically acquire lock before write, release after
    - Return error if file locked by another agent
    - _Requirements: 6.1, 6.2_

  - [ ]* 5.3 Write property test for lock mutual exclusion
    - **Property 6: Advisory Lock Mutual Exclusion**
    - **Validates: Requirements 6.1, 6.2**

  - [ ]* 5.4 Write property test for tool-enforced locking
    - **Property 29: Tool-Enforced File Locking**
    - **Validates: Requirements 6.1, 6.2**

  - [ ] 5.5 Implement stale lock cleanup
    - Detect locks held by crashed agents (no heartbeat for 30s)
    - Auto-release stale locks
    - _Requirements: 6.4_

  - [ ]* 5.6 Write property test for stale lock cleanup
    - **Property 8: Stale Lock Cleanup**
    - **Validates: Requirements 6.4**

- [ ] 6. Checkpoint - Verify locking mechanisms
  - Ensure all tests pass, ask the user if questions arise.

### Phase 2: Isolation

- [ ] 7. Implement Isolation Mode Detection
  - [ ] 7.1 Create AgentNamespaceManager with capability detection
    - Detect if running in Sysbox (check for nested namespace support)
    - Set IsolationMode to Full or Degraded based on capabilities
    - Log isolation mode at startup
    - _Requirements: 9.1_

  - [ ] 7.2 Implement degraded mode fallback
    - Use process groups instead of namespaces
    - Allocate random ports in configured range
    - Warn user about reduced isolation
    - _Requirements: 9.2_

  - [ ]* 7.3 Write property test for degraded mode port allocation
    - **Property 30: Degraded Mode Port Allocation**
    - **Validates: Requirements 9.2**

- [ ] 8. Implement Network Namespace Isolation (Full Mode)
  - [ ] 8.1 Create network namespace for agent
    - Execute `ip netns add <agent-name>`
    - Bring up loopback interface inside namespace
    - _Requirements: 9.1_

  - [ ] 8.2 Implement namespace execution wrapper
    - Execute commands via `ip netns exec <name> <command>`
    - _Requirements: 9.1_

  - [ ] 8.3 Delete namespace on agent removal
    - Execute `ip netns delete <agent-name>`
    - _Requirements: 9.5_

- [ ] 9. Implement Mount Namespace Isolation (Full Mode)
  - [ ] 9.1 Set up mount namespace with bind-mounted worktree
    - Use `unshare -m` to create mount namespace
    - Bind-mount worktree as agent's visible root
    - Prevent traversal to other agent directories
    - _Requirements: 9.1_

  - [ ]* 9.2 Write property test for mount namespace isolation
    - **Property 26: Mount Namespace Isolation**
    - **Validates: Requirements 9.1**

- [ ] 10. Implement PID Namespace Isolation (Full Mode)
  - [ ] 10.1 Create PID namespace for agent processes
    - Use `unshare -p` to create PID namespace
    - Ensure agent cannot signal other agents' processes
    - _Requirements: 9.1_

  - [ ]* 10.2 Write property test for PID namespace isolation
    - **Property 27: PID Namespace Isolation**
    - **Validates: Requirements 9.1**

- [ ] 11. Implement Cgroup-Based Process Management
  - [ ] 11.1 Create cgroup hierarchy for agents
    - Create `/workspace/.polis/cgroups/<agent-name>/`
    - Set memory limit from agent config
    - _Requirements: 11.1, 11.2_

  - [ ]* 11.2 Write property test for memory limit application
    - **Property 14: Memory Limit Application**
    - **Validates: Requirements 11.1, 11.2**

  - [ ] 11.3 Implement process reaping via cgroup
    - On agent removal, kill all processes in cgroup
    - Handle detached/background processes
    - _Requirements: 4.1_

  - [ ]* 11.4 Write property test for process reaping
    - **Property 28: Process Reaping on Removal**
    - **Validates: Requirements 4.1**

  - [ ] 11.5 Implement OOM handling
    - Detect when agent exceeds memory limit
    - Terminate agent and log event
    - _Requirements: 11.4_

  - [ ]* 11.6 Write property test for memory limit enforcement
    - **Property 15: Memory Limit Enforcement**
    - **Validates: Requirements 11.4**

- [ ] 12. Checkpoint - Verify isolation mechanisms
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 13. Implement Port Manager
  - [ ] 13.1 Create PortManager with port allocation
    - Track allocated ports per agent
    - Allocate from range 3001-9999
    - _Requirements: 9.2, 9.4_

  - [ ]* 13.2 Write property test for port mapping uniqueness
    - **Property 9: Port Mapping Uniqueness**
    - **Validates: Requirements 9.2**

  - [ ]* 13.3 Write property test for port range validity
    - **Property 10: Port Range Validity**
    - **Validates: Requirements 9.4**

  - [ ] 13.4 Implement socat port forwarding
    - Bridge internal namespace port to external workspace port
    - Handle TCP and UDP protocols
    - _Requirements: 9.2_

  - [ ] 13.5 Clean up port mappings on agent removal
    - Kill socat processes
    - Release allocated ports
    - _Requirements: 9.5_

- [ ] 14. Implement Agent Removal with Full Cleanup
  - [ ] 14.1 Integrate all cleanup steps in remove_agent()
    - Reap processes via cgroup
    - Release advisory locks
    - Remove port mappings
    - Delete namespaces
    - Remove worktree
    - Update state.json
    - _Requirements: 4.1, 4.2, 4.3, 4.5, 9.5_

  - [ ]* 14.2 Write property test for agent removal cleanup
    - **Property 4: Agent Removal Cleanup**
    - **Validates: Requirements 4.1, 4.2, 4.3, 4.5, 9.5**

- [ ] 15. Checkpoint - Verify full isolation and cleanup
  - Ensure all tests pass, ask the user if questions arise.

### Phase 3: CLI & TUI Integration

- [ ] 16. Extend polis-cli agents command
  - [ ] 16.1 Implement `polis agents add` command
    - Parse --base, --branch, --memory flags
    - Call WorkspaceManager.create_agent()
    - Display success message with agent details
    - _Requirements: 1.1, 1.2, 1.4, 1.7_

  - [ ] 16.2 Implement `polis agents remove` command
    - Parse --force flag
    - Check for uncommitted changes (prompt if not --force)
    - Call WorkspaceManager.remove_agent()
    - _Requirements: 4.1, 4.4, 4.6_

  - [ ]* 16.3 Write property test for non-existent agent error
    - **Property 5: Non-Existent Agent Error**
    - **Validates: Requirements 3.2, 4.6**

  - [ ] 16.4 Implement `polis agents list` command
    - Display table with NAME, STATUS, BRANCH, PORT, MEMORY columns
    - Show isolation mode indicator
    - _Requirements: 2.1, 2.2, 2.5_

  - [ ] 16.5 Implement `polis agents locks` command
    - Display all active file locks with holder info
    - _Requirements: 6.6_

- [ ] 17. Implement Agent Task Assignment
  - [ ] 17.1 Implement `polis agents run` command
    - Parse agent name and prompt
    - Auto-start agent if stopped
    - Send prompt to agent process
    - Return immediately (background execution)
    - _Requirements: 3.1, 3.3, 3.4_

  - [ ]* 17.2 Write property test for agent auto-start
    - **Property 24: Agent Auto-Start on Task**
    - **Validates: Requirements 3.3**

  - [ ] 17.3 Implement task logging
    - Log task assignment with timestamp to agent log file
    - _Requirements: 3.5_

  - [ ]* 17.4 Write property test for task logging
    - **Property 25: Task Logging**
    - **Validates: Requirements 3.5**

- [ ] 18. Implement Agent Output Streaming
  - [ ] 18.1 Implement `polis agents watch` command
    - Stream agent stdout/stderr to terminal
    - Add timestamps to output lines
    - Handle Ctrl+C to detach without stopping agent
    - _Requirements: 5.1, 5.2, 5.3_

  - [ ] 18.2 Implement output history for stopped agents
    - Store last 50 lines of output per agent
    - Display history when watching stopped agent
    - _Requirements: 5.4_

  - [ ]* 18.3 Write property test for concurrent watchers
    - **Property: Concurrent watchers don't interfere**
    - **Validates: Requirements 5.5**

- [ ] 19. Implement Agent Shell Integration
  - [ ] 19.1 Implement `polis shell <agent-name>` command
    - Enter agent's worktree directory
    - Enter agent's network namespace (Full mode)
    - Use Cash shell with governance policies
    - _Requirements: 13.1, 13.2, 13.5_

  - [ ]* 19.2 Write property test for shell context consistency
    - **Property 18: Shell Context Consistency**
    - **Validates: Requirements 13.1, 13.2, 13.3**

  - [ ] 19.3 Implement shell exit restoration
    - Return to main workspace context on exit
    - _Requirements: 13.4_

  - [ ]* 19.4 Write property test for shell exit restoration
    - **Property 19: Shell Exit Restoration**
    - **Validates: Requirements 13.4**

- [ ] 20. Checkpoint - Verify CLI commands
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 21. Implement Agent-to-PR Workflow
  - [ ] 21.1 Implement `polis agents pr` command
    - Check for uncommitted changes (prompt to commit)
    - Generate PR description from commit messages
    - Create PR via GitHub API
    - Display PR URL
    - _Requirements: 7.1, 7.2, 7.4_

  - [ ]* 21.2 Write property test for PR creation
    - **Property: PR created with correct branches**
    - **Validates: Requirements 7.1, 7.2**

- [ ] 22. Implement TUI Multi-Agent View
  - [ ] 22.1 Add agent sidebar to TUI
    - List all agents with status indicators
    - Support click/keyboard selection
    - _Requirements: 8.1_

  - [ ] 22.2 Implement agent terminal switching
    - Switch main view to selected agent's tmux session
    - _Requirements: 8.2_

  - [ ] 22.3 Implement grid view for multiple agents
    - Support 2x2 grid showing 4 agent terminals
    - _Requirements: 8.3_

  - [ ] 22.4 Implement unified log panel
    - Aggregate high-level events from all agents
    - Display at bottom of TUI
    - _Requirements: 8.4_

  - [ ] 22.5 Implement keyboard shortcuts
    - Alt+1 through Alt+9 for agent switching
    - _Requirements: 8.5_

  - [ ] 22.6 Implement broadcast mode
    - Send command to all running agents
    - _Requirements: 8.6_

  - [ ]* 22.7 Write property test for broadcast delivery
    - **Property 23: Broadcast Command Delivery**
    - **Validates: Requirements 8.6**

- [ ] 23. Implement Dependency Deduplication
  - [ ] 23.1 Configure pnpm as default for Node.js projects
    - Detect Node.js projects in worktrees
    - Set up shared pnpm store
    - _Requirements: 12.1_

  - [ ] 23.2 Implement npm to pnpm interception in Cash shell
    - Intercept `npm install` commands
    - Redirect to `pnpm install`
    - _Requirements: 12.4_

  - [ ]* 23.3 Write property test for npm interception
    - **Property 17: npm to pnpm Interception**
    - **Validates: Requirements 12.4**

  - [ ]* 23.4 Write property test for dependency deduplication
    - **Property 16: Dependency Deduplication**
    - **Validates: Requirements 12.2**

- [ ] 24. Final Checkpoint - Full integration testing
  - Ensure all tests pass, ask the user if questions arise.
  - Run end-to-end test with 10 concurrent agents
  - Verify performance targets (10s startup, <100MB overhead)

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties
- Unit tests validate specific examples and edge cases
- Phase 1 can be tested without Sysbox using mocked namespaces
- Phase 2 requires Sysbox or privileged Docker for full testing
- Phase 3 TUI tasks can be parallelized with CLI tasks
