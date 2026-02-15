# Requirements Document

## Introduction

This document specifies the requirements for the Multi-Agent Workspaces feature in the Polis platform. This feature enables 2-10 AI coding agents to work concurrently on the same repository within a single Polis workspace container, using a "Namespace-Isolated Worktree" architecture that combines Git worktrees for filesystem isolation, Linux network namespaces for port isolation, and centralized coordination for safe concurrent operations.

## Glossary

- **Agent**: An AI coding assistant (e.g., Claude, Gemini, Kiro, Aider) running within the Polis workspace
- **Worktree**: A Git worktree providing a dedicated directory and git index for an agent
- **Network_Namespace**: A Linux network namespace providing isolated network stack per agent
- **Workspace_Manager**: The polis-server component responsible for managing agent worktrees and coordination
- **Git_Mutex**: A centralized locking mechanism for serializing git metadata operations
- **Port_Manager**: Component that maps internal namespace ports to unique external ports
- **Advisory_Lock**: A non-blocking file lock used to prevent concurrent edits to the same file
- **Bare_Repository**: The shared `.git` object database used by all worktrees
- **Cash_Shell**: The Polis governance-aware shell (polis-shell) that intercepts and validates commands

## Requirements

### Requirement 1: Agent Creation

**User Story:** As a developer, I want to create new AI coding agents with dedicated worktrees, so that multiple agents can work on different branches simultaneously without file conflicts.

#### Acceptance Criteria

1. WHEN a user executes `polis agents add <name> --base=<agent-type>`, THE Workspace_Manager SHALL create a new git worktree at `/workspace/.polis/worktrees/<name>`
2. WHEN a user executes `polis agents add <name> --branch=<branch-name>`, THE Workspace_Manager SHALL create the worktree on the specified branch
3. IF the agent name already exists, THEN THE CLI SHALL return an error message indicating the name is taken
4. IF the base agent type is not available in the workspace, THEN THE CLI SHALL return an error listing available agent types
5. WHEN a worktree is created, THE Workspace_Manager SHALL complete the operation within 10 seconds
6. WHEN a worktree is created, THE Workspace_Manager SHALL create a corresponding network namespace for the agent
7. THE Workspace_Manager SHALL support creating up to 10 concurrent agents
8. WHEN an agent is created without specifying a branch, THE Workspace_Manager SHALL create a new branch named `agent/<name>` from the current HEAD

### Requirement 2: Agent Listing

**User Story:** As a developer, I want to list all active agents with their status, so that I can monitor what each agent is working on.

#### Acceptance Criteria

1. WHEN a user executes `polis agents list`, THE CLI SHALL display all configured agents with their current status
2. WHEN displaying agent status, THE CLI SHALL show: agent name, status (Running/Idle/Stopped), current branch, and mapped port
3. WHEN an agent has a running process, THE CLI SHALL indicate the process type (e.g., dev server, build)
4. WHEN displaying resource usage, THE CLI SHALL show memory consumption per agent
5. THE CLI SHALL format output in a table with columns: NAME, STATUS, BRANCH, PORT, MEMORY

### Requirement 3: Task Assignment

**User Story:** As a developer, I want to assign tasks to specific agents via prompts, so that I can delegate coding work to AI assistants.

#### Acceptance Criteria

1. WHEN a user executes `polis agents run <name> "<prompt>"`, THE CLI SHALL send the prompt to the specified agent
2. IF the specified agent does not exist, THEN THE CLI SHALL return an error with available agent names
3. IF the specified agent is not running, THEN THE CLI SHALL start the agent before sending the prompt
4. WHEN a task is assigned, THE CLI SHALL return immediately and run the agent in the background
5. WHEN a task is assigned, THE Workspace_Manager SHALL log the task assignment with timestamp

### Requirement 4: Agent Removal

**User Story:** As a developer, I want to remove agents and clean up their resources, so that I can free up system resources when agents are no longer needed.

#### Acceptance Criteria

1. WHEN a user executes `polis agents remove <name>`, THE Workspace_Manager SHALL terminate any running processes for that agent
2. WHEN removing an agent, THE Workspace_Manager SHALL delete the git worktree at `/workspace/.polis/worktrees/<name>`
3. WHEN removing an agent, THE Workspace_Manager SHALL destroy the associated network namespace
4. IF the agent has uncommitted changes, THEN THE CLI SHALL prompt for confirmation before removal
5. WHEN an agent is removed, THE Workspace_Manager SHALL release any advisory locks held by that agent
6. IF the agent does not exist, THEN THE CLI SHALL return an error indicating the agent was not found

### Requirement 5: Agent Output Streaming

**User Story:** As a developer, I want to watch an agent's output in real-time, so that I can monitor progress and intervene if needed.

#### Acceptance Criteria

1. WHEN a user executes `polis agents watch <name>`, THE CLI SHALL stream the agent's stdout and stderr to the terminal
2. WHEN watching an agent, THE CLI SHALL display output with timestamps
3. WHEN the user presses Ctrl+C while watching, THE CLI SHALL detach from the stream without stopping the agent
4. IF the agent is not running, THEN THE CLI SHALL display the last 50 lines of output history
5. WHEN multiple users watch the same agent, THE CLI SHALL support concurrent watchers without interference

### Requirement 6: File Conflict Prevention

**User Story:** As a developer, I want agents to coordinate file access, so that two agents don't simultaneously edit the same file and cause conflicts.

#### Acceptance Criteria

1. WHEN an agent opens a file for editing, THE Workspace_Manager SHALL acquire an advisory lock on that file
2. WHEN another agent attempts to edit a locked file, THE Workspace_Manager SHALL notify the agent that the file is locked by another agent
3. WHEN an agent releases a file, THE Workspace_Manager SHALL release the advisory lock
4. IF an agent crashes while holding locks, THEN THE Workspace_Manager SHALL release those locks within 30 seconds
5. THE Advisory_Lock system SHALL support lock acquisition within 100ms under normal load
6. WHEN displaying lock status, THE CLI SHALL show which agent holds each lock via `polis agents locks`

### Requirement 7: Agent-to-PR Workflow

**User Story:** As a developer, I want to create pull requests from an agent's branch, so that I can review and merge the agent's work.

#### Acceptance Criteria

1. WHEN a user executes `polis agents pr <name>`, THE CLI SHALL create a pull request from the agent's branch to the base branch
2. WHEN creating a PR, THE CLI SHALL use the agent's commit messages to generate a PR description
3. IF the agent has uncommitted changes, THEN THE CLI SHALL prompt to commit them before creating the PR
4. WHEN a PR is created, THE CLI SHALL display the PR URL
5. IF the remote repository is not configured, THEN THE CLI SHALL return an error with setup instructions

### Requirement 8: TUI Multi-Agent View

**User Story:** As a developer, I want to manage all agents from a unified TUI interface, so that I can efficiently coordinate multiple agents.

#### Acceptance Criteria

1. WHEN the TUI is opened, THE TUI SHALL display a sidebar listing all active agents
2. WHEN a user selects an agent in the sidebar, THE TUI SHALL switch the main terminal view to that agent's session
3. THE TUI SHALL support a grid view showing up to 4 agent terminals simultaneously
4. WHEN an agent produces output, THE TUI SHALL update the unified log panel at the bottom
5. THE TUI SHALL support keyboard shortcuts for switching between agents (Alt+1 through Alt+9)
6. WHEN a user executes a broadcast command, THE TUI SHALL send the command to all running agents

### Requirement 9: Network Namespace Isolation

**User Story:** As a developer, I want each agent to have isolated network access, so that multiple agents can bind to the same ports without conflicts.

#### Acceptance Criteria

1. WHEN an agent is created, THE Workspace_Manager SHALL create a dedicated network namespace using `unshare -n` or `ip netns`
2. WHEN an agent binds to a port (e.g., 3000), THE Port_Manager SHALL map it to a unique external port (e.g., 3001, 3002)
3. WHEN displaying port mappings, THE CLI SHALL show internal and external port pairs
4. THE Port_Manager SHALL support mapping ports in the range 3000-9999
5. WHEN an agent is removed, THE Workspace_Manager SHALL clean up all port forwarding rules
6. WHEN the workspace starts, THE Workspace_Manager SHALL restore port mappings for existing agents

### Requirement 10: Git Operation Coordination

**User Story:** As a developer, I want git operations to be safely coordinated across agents, so that concurrent commits and branch operations don't corrupt the repository.

#### Acceptance Criteria

1. WHEN an agent executes a git metadata operation (commit, branch, fetch, gc), THE Git_Mutex SHALL serialize the operation
2. THE Git_Mutex SHALL acquire locks within 100ms under normal load
3. IF a lock cannot be acquired within 5 seconds, THEN THE Git_Mutex SHALL return a timeout error
4. WHEN multiple agents commit simultaneously, THE Git_Mutex SHALL ensure commits are applied sequentially
5. THE Git_Mutex SHALL NOT lock for standard file operations (git add, git diff, git status)
6. WHEN the workspace shuts down, THE Git_Mutex SHALL release all held locks

### Requirement 11: Resource Management

**User Story:** As a developer, I want to monitor and limit agent resource usage, so that agents don't consume excessive system resources.

#### Acceptance Criteria

1. WHEN an agent is created, THE Workspace_Manager SHALL allocate a default memory limit of 512MB
2. WHEN a user specifies `--memory=<limit>`, THE Workspace_Manager SHALL apply the specified memory limit
3. THE Workspace_Manager SHALL track memory usage per agent and expose it via `polis agents list`
4. IF an agent exceeds its memory limit, THEN THE Workspace_Manager SHALL terminate the agent and log the event
5. THE total memory overhead per agent SHALL NOT exceed 100MB for the isolation infrastructure
6. WHEN 10 agents are running, THE total additional memory overhead SHALL NOT exceed 1GB

### Requirement 12: Dependency Deduplication

**User Story:** As a developer, I want dependencies to be shared across agent worktrees, so that disk space is used efficiently.

#### Acceptance Criteria

1. WHEN using Node.js projects, THE Workspace_Manager SHALL configure pnpm as the default package manager
2. WHEN multiple agents install the same dependency version, THE dependency SHALL be stored once and hard-linked
3. THE Workspace_Manager SHALL reduce disk usage by at least 70% compared to duplicated node_modules
4. WHEN an agent runs `npm install`, THE Cash_Shell SHALL intercept and redirect to `pnpm install`
5. THE Workspace_Manager SHALL support dependency deduplication for npm, pip, and cargo packages

### Requirement 13: Agent Shell Integration

**User Story:** As a developer, I want to enter an agent's context directly, so that I can debug or manually intervene in an agent's environment.

#### Acceptance Criteria

1. WHEN a user executes `polis shell <agent-name>`, THE CLI SHALL drop the user into the agent's worktree directory
2. WHEN entering an agent's shell, THE CLI SHALL also enter the agent's network namespace
3. WHEN in an agent's shell, THE user SHALL see the same filesystem and network state as the agent
4. WHEN the user exits the shell, THE CLI SHALL return to the main workspace context
5. THE shell session SHALL use the Cash_Shell with governance policies applied

### Requirement 14: Workspace Initialization

**User Story:** As a developer, I want the multi-agent infrastructure to be set up automatically, so that I can start using agents without manual configuration.

#### Acceptance Criteria

1. WHEN `polis up` is executed in a repository, THE Workspace_Manager SHALL initialize the bare repository at `/workspace/.polis/bare-repo.git`
2. WHEN initializing, THE Workspace_Manager SHALL create the worktrees directory at `/workspace/.polis/worktrees/`
3. IF the repository already has a `.polis` directory, THEN THE Workspace_Manager SHALL preserve existing agent configurations
4. WHEN initialization completes, THE Workspace_Manager SHALL verify the git worktree setup is functional
5. THE initialization process SHALL complete within 30 seconds for repositories up to 1GB

### Requirement 15: Agent Persistence

**User Story:** As a developer, I want agent configurations to persist across workspace restarts, so that I don't have to recreate agents each time.

#### Acceptance Criteria

1. WHEN the workspace is stopped, THE Workspace_Manager SHALL save agent configurations to `/workspace/.polis/agents.json`
2. WHEN the workspace is started, THE Workspace_Manager SHALL restore agents from the saved configuration
3. WHEN restoring agents, THE Workspace_Manager SHALL recreate network namespaces and port mappings
4. IF an agent's worktree is corrupted, THEN THE Workspace_Manager SHALL log an error and skip that agent
5. THE agent configuration file SHALL include: name, base type, branch, port mappings, and memory limit

While editing files you MUST split each edit into max 50 lines