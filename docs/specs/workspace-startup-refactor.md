# Workspace Startup Refactor Specification

## Problem Statement

A race condition exists between systemd's `polis.service` and the CLI's `start_compose()` function. Both attempt to start Docker containers, causing:

1. **Fresh install**: Containers start twice, causing "removal already in progress" errors
2. **Agent switch** (`polis stop` → `polis start --agent=X`): Service starts with OLD agent before CLI updates config
3. **Agent removal** (`polis stop` → `polis start`): Service starts with stale agent overlay

### Root Cause

```
VM starts → systemd reaches multi-user.target → polis.service auto-starts
                    ↓ (race)
            CLI connects → tries to configure → calls start_compose()
```

## Solution: Ready Gate Pattern

Use systemd's `ConditionPathExists=` directive to create a gate that CLI controls:

```
/opt/polis/.ready
  ├── exists     → systemd can auto-start (host reboot scenario)
  └── missing    → CLI is managing startup, systemd skips
```

## Design Patterns

| Pattern | Usage |
|---------|-------|
| **Gate/Condition** | `.ready` file synchronizes CLI and systemd |
| **Command** | Each operation is discrete and composable |
| **Template Method** | Startup sequence varies by scenario |

## SOLID Principles

| Principle | Application |
|-----------|-------------|
| **SRP** | `polis.service` handles auto-start only; CLI handles controlled operations |
| **OCP** | New agents don't change startup mechanism |
| **DIP** | Application depends on `ShellExecutor` trait, not concrete implementation |

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  DOMAIN LAYER                                                   │
│  domain/workspace.rs                                            │
│    + ACTIVE_OVERLAY_PATH: &str                                  │
│    + READY_MARKER_PATH: &str                                    │
│                                                                 │
│  domain/agent/mod.rs                                            │
│    + overlay_path(agent_name: &str) -> String                   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  APPLICATION LAYER                                              │
│  services/workspace_start.rs                                    │
│    + set_active_overlay(path: Option<&str>)                     │
│    + set_ready_marker(enabled: bool)                            │
│    ~ create_and_start_vm() - remove start_compose()             │
│    ~ restart_vm() - remove start_compose()                      │
│    ~ handle_running_vm() - keep start_compose()                 │
│                                                                 │
│  services/workspace_stop.rs                                     │
│    ~ stop_workspace() - clear ready marker before stop          │
│                                                                 │
│  services/agent_crud.rs                                         │
│    ~ remove_agent() - clear symlink if active agent removed     │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  INFRASTRUCTURE LAYER                                           │
│  cloud-init.yaml                                                │
│    ~ polis.service - add ConditionPathExists, update ExecStart  │
└─────────────────────────────────────────────────────────────────┘
```

## Detailed Changes

### 1. Domain Layer

**File: `cli/src/domain/workspace.rs`**

Add constants:
```rust
/// Path to active compose overlay symlink inside VM.
pub const ACTIVE_OVERLAY_PATH: &str = "/opt/polis/compose.active.yaml";

/// Path to ready marker file inside VM.
/// When present, polis.service is allowed to auto-start.
/// CLI removes this before controlled restarts.
pub const READY_MARKER_PATH: &str = "/opt/polis/.ready";
```

**File: `cli/src/domain/agent/mod.rs`**

Add pure function:
```rust
/// Returns the path to an agent's compose overlay file.
#[must_use]
pub fn overlay_path(agent_name: &str) -> String {
    format!("{}/agents/{agent_name}/.generated/compose.agent.yaml", super::workspace::VM_ROOT)
}
```

### 2. Application Layer

**File: `cli/src/application/services/workspace_start.rs`**

Add helper functions:
```rust
use crate::domain::workspace::{ACTIVE_OVERLAY_PATH, READY_MARKER_PATH};

/// Set or remove the active compose overlay symlink.
async fn set_active_overlay(
    provisioner: &impl ShellExecutor,
    overlay_path: Option<&str>,
) -> Result<()> {
    match overlay_path {
        Some(path) => {
            provisioner
                .exec(&["ln", "-sf", path, ACTIVE_OVERLAY_PATH])
                .await
                .context("creating overlay symlink")?;
        }
        None => {
            provisioner
                .exec(&["rm", "-f", ACTIVE_OVERLAY_PATH])
                .await
                .context("removing overlay symlink")?;
        }
    }
    Ok(())
}

/// Set or clear the ready marker that gates polis.service auto-start.
async fn set_ready_marker(provisioner: &impl ShellExecutor, enabled: bool) -> Result<()> {
    if enabled {
        provisioner
            .exec(&["touch", READY_MARKER_PATH])
            .await
            .context("creating ready marker")?;
    } else {
        provisioner
            .exec(&["rm", "-f", READY_MARKER_PATH])
            .await
            .context("removing ready marker")?;
    }
    Ok(())
}
```

**Modify `create_and_start_vm()`:**
```rust
async fn create_and_start_vm(...) -> Result<()> {
    // ... existing steps 1-6 ...

    // Step 7: Set up agent if requested.
    let overlay = if let Some(name) = agent {
        reporter.begin_stage(&format!("installing agent '{name}'..."));
        setup_agent(provisioner, local_fs, name, &envs).await?;
        Some(crate::domain::agent::overlay_path(name))
    } else {
        None
    };

    // Step 8: Set active overlay symlink.
    set_active_overlay(provisioner, overlay.as_deref()).await?;

    // Step 9: Enable ready marker and start services.
    set_ready_marker(provisioner, true).await?;
    // polis.service is started via systemctl, not start_compose()
    provisioner.exec(&["sudo", "systemctl", "start", "polis"]).await?;

    // Step 10: Wait for health.
    // ... rest unchanged ...
}
```

**Modify `restart_vm()`:**

> **Note:** `vm::restart()` internally calls `start_services()` → `systemctl start polis`.
> Because `stop_workspace()` already cleared `.ready`, the `ConditionPathExists` check
> causes that internal `systemctl start polis` to be a no-op. We then set the overlay,
> mark ready, and explicitly start the service ourselves.

```rust
async fn restart_vm(...) -> Result<()> {
    // vm::restart() calls start_services internally, but .ready was cleared
    // during stop_workspace(), so systemd's ConditionPathExists fails → no-op.
    vm::restart(provisioner, reporter, false).await?;

    reporter.begin_stage("verifying components...");
    pull_images(provisioner, reporter).await?;

    let overlay = if let Some(name) = agent {
        reporter.begin_stage(&format!("installing agent '{name}'..."));
        setup_agent(provisioner, local_fs, name, &envs).await?;
        Some(crate::domain::agent::overlay_path(name))
    } else {
        None
    };

    // Update overlay symlink, then gate-open and start services.
    set_active_overlay(provisioner, overlay.as_deref()).await?;
    set_ready_marker(provisioner, true).await?;
    provisioner.exec(&["sudo", "systemctl", "start", "polis"]).await?;

    // ... save state ...
}
```

**Modify `handle_running_vm()`:**

Keep `start_compose()` here - VM is already running, systemd won't trigger:
```rust
async fn handle_running_vm(...) -> Result<StartOutcome> {
    // ... existing checks ...

    if current_agent.is_none() && let Some(name) = agent {
        reporter.begin_stage(&format!("installing agent '{name}'..."));
        setup_agent(provisioner, local_fs, name, &envs).await?;
        
        // Update symlink for future reboots
        let overlay = crate::domain::agent::overlay_path(name);
        set_active_overlay(provisioner, Some(&overlay)).await?;
        
        // Start with overlay - VM already running, use compose directly
        start_compose(provisioner, Some(name)).await?;
        
        // ... rest unchanged ...
    }
}
```

**File: `cli/src/application/services/workspace_stop.rs`**

Clear ready marker before stopping:
```rust
pub async fn stop_workspace(...) -> Result<StopOutcome> {
    match vm::state(provisioner).await? {
        VmState::Running => {
            // Clear ready marker so polis.service won't auto-start on next boot.
            // CLI will set it again after controlled startup.
            let _ = provisioner
                .exec(&["rm", "-f", crate::domain::workspace::READY_MARKER_PATH])
                .await;
            
            vm::stop(provisioner).await?;
            Ok(StopOutcome::Stopped)
        }
        // ... rest unchanged ...
    }
}
```

**File: `cli/src/application/services/agent_crud.rs`**

Clear symlink when removing active agent:
```rust
pub async fn remove_agent(...) -> Result<()> {
    // ... existing validation ...

    let active = state_mgr.load_async().await?.and_then(|s| s.active_agent);
    
    if active.as_deref() == Some(agent_name) {
        // Stop services, remove symlink
        // ... existing stop logic ...
        
        provisioner
            .exec(&["rm", "-f", crate::domain::workspace::ACTIVE_OVERLAY_PATH])
            .await?;
    }

    // ... rest unchanged ...
}
```

### 3. Infrastructure Layer

**File: `cloud-init.yaml`**

Update polis.service:
```yaml
- path: /etc/systemd/system/polis.service
  content: |
    [Unit]
    Description=Polis Workspace Services
    After=network-online.target docker.service
    Wants=network-online.target
    Requires=docker.service
    # Only auto-start if CLI has marked system ready.
    # CLI removes .ready before controlled restarts.
    ConditionPathExists=/opt/polis/.ready

    [Service]
    Type=oneshot
    RemainAfterExit=yes
    # Use overlay if symlink exists, otherwise base compose only.
    ExecStart=/bin/bash -c 'cd /opt/polis && if [ -f compose.active.yaml ]; then docker compose -f docker-compose.yml -f compose.active.yaml up -d --remove-orphans; else docker compose -f docker-compose.yml up -d --remove-orphans; fi'
    ExecStop=/bin/bash -c 'cd /opt/polis && if [ -f compose.active.yaml ]; then docker compose -f docker-compose.yml -f compose.active.yaml down; else docker compose -f docker-compose.yml down; fi'
    Restart=on-failure
    User=ubuntu
    WorkingDirectory=/opt/polis

    [Install]
    WantedBy=multi-user.target
```

## Flow Diagrams

### Fresh Install (NotFound → Running)

```
1. CLI: create VM via cloud-init
2. cloud-init: installs polis.service, .ready doesn't exist
3. systemd: ConditionPathExists fails → polis.service skipped
4. CLI: transfer config, generate certs, pull images
5. CLI: setup agent (if any), create symlink
6. CLI: touch .ready
7. CLI: systemctl start polis
8. Containers start with correct config
```

### Controlled Restart (Stopped → Running)

```
1. [Previously] CLI: rm .ready (during stop)
2. CLI: vm::restart() starts VM, internal systemctl start polis is a no-op
   (ConditionPathExists fails because .ready was cleared in step 1)
3. CLI: pull images, setup agent (if changed)
4. CLI: update symlink (or remove if no agent)
5. CLI: touch .ready
6. CLI: systemctl start polis (explicit, after overlay is set)
7. Containers start with correct config
```

### Host Reboot (Uncontrolled)

```
1. Host reboots
2. Multipass auto-starts polis VM
3. systemd: .ready exists → polis.service starts
4. polis.service: uses existing symlink
5. Containers start with last-known agent config
```

### Add Agent to Running VM

```
1. VM already running with no agent
2. CLI: polis start --agent=openclaw
3. CLI: setup agent, create symlink
4. CLI: docker compose up -d (direct, not via systemd)
5. Containers updated with agent overlay
```

## Testing Checklist

- [ ] Fresh install without agent
- [ ] Fresh install with agent
- [ ] Stop → Start (same agent)
- [ ] Stop → Start (different agent)
- [ ] Stop → Start (remove agent)
- [ ] Host reboot preserves agent
- [ ] Add agent to running workspace
- [ ] Remove active agent

## Migration Notes

Existing installations will not have `.ready` file. On first `polis start` after update:
- VM starts, polis.service condition fails (no .ready)
- CLI creates .ready and starts services
- Subsequent host reboots work correctly

No manual migration required.
