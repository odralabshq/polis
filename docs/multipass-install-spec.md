# Multipass Installation & Pre-flight Specification

## Overview

Polis uses Multipass to launch a local VM from a `file://` URL. This feature
requires Multipass ≥ 1.16.0 on all platforms. This spec defines the unified
installation and pre-flight check flow for Linux, macOS, and Windows.

---

## Platform Requirements

| Platform | Hypervisor | Min OS | Min Multipass |
|---|---|---|---|
| Linux (x86_64, arm64) | QEMU/KVM via snap | snapd available | 1.16.0 |
| macOS (Intel + Apple Silicon) | QEMU (Hypervisor.framework) | macOS 13 Ventura | 1.16.0 |
| Windows 10/11 Pro/Enterprise | Hyper-V | Windows 10 1803 | 1.16.0 |
| Windows 10/11 Home | VirtualBox (must be pre-installed) | Windows 10 | 1.16.0 |

**Hardware minimums:** 64-bit CPU with hardware virtualisation (VT-x / AMD-V /
Apple Silicon), 8 GB RAM, 10 GB free disk.

---

## GitHub Release Asset Names

URL pattern:
`https://github.com/canonical/multipass/releases/download/v{VERSION}/{ASSET}`

| Platform | Asset filename |
|---|---|
| Linux x86_64 | `multipass_{VERSION}_amd64.snap` |
| Linux arm64 | `multipass_{VERSION}_arm64.snap` |
| macOS | `multipass-{VERSION}+mac-Darwin.pkg` |
| Windows | `multipass-{VERSION}+win-Windows.msi` |

Version is resolved from the GitHub releases API (`/releases/latest`).

---

## Installation Flow

### Entry point: `polis install` (or first-run check inside `polis run`)

```
detect OS + arch
│
├─ multipass present? (multipass version 2>/dev/null)
│   ├─ YES → version ≥ 1.16.0?
│   │         ├─ YES → post-install config (see below)
│   │         └─ NO  → WARN: "Multipass {X} found, need ≥ 1.16.0"
│   │                  offer auto-upgrade (same install path)
│   └─ NO  → install multipass (see per-platform below)
│
└─ post-install config
```

### Linux

```
snapd present? (command -v snap)
├─ NO  → ERROR: "snapd is required. Install it for your distro:
│         https://snapcraft.io/docs/installing-snapd"
│         EXIT 1
└─ YES → download multipass_{VERSION}_amd64.snap (or arm64)
          sudo snap install multipass_*.snap --dangerous
          sudo snap connect multipass:removable-media
          verify socket group membership (see Post-install: Linux)
```

### macOS

```
download multipass-{VERSION}+mac-Darwin.pkg to /tmp
sudo installer -pkg /tmp/multipass-*.pkg -target /
verify: multipass version
```

### Windows (`install.ps1`)

```
Hyper-V available?
(Get-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All).State -eq "Enabled"
├─ YES → download multipass-{VERSION}+win-Windows.msi
│         Start-Process msiexec -ArgumentList "/i multipass-*.msi /quiet" -Wait
└─ NO  → VirtualBox installed?
          (Test-Path "C:\Program Files\Oracle\VirtualBox\VBoxManage.exe")
          ├─ YES → install .msi (same as above)
          │         multipass set local.driver=virtualbox
          │         WARN if WSL2 is active:
          │           wsl --status 2>$null → "WSL2 detected: VirtualBox may
          │           run slowly. Consider enabling Hyper-V or disabling WSL2."
          └─ NO  → ERROR: "Hyper-V is not available on this Windows edition.
                    Install VirtualBox first:
                    https://www.virtualbox.org/wiki/Downloads
                    Then re-run: polis install"
                    EXIT 1
```

---

## Post-install Config

### Linux (snap only)

1. Connect `removable-media` interface (required for `file://` image access):
   ```
   snap list multipass  # confirm snap install
   snap connections multipass | grep "removable-media.*:removable-media"
   ├─ connected → skip
   └─ not connected → sudo snap connect multipass:removable-media
   ```

2. Socket group membership:
   ```
   SOCKET_GROUP=$(stat -c '%G' /var/snap/multipass/common/multipass_socket)
   groups | grep -q "$SOCKET_GROUP"
   ├─ member → skip
   └─ not member → WARN: "Run: sudo usermod -aG {group} $USER
                    Then log out and back in, or run: newgrp {group}"
   ```

### macOS

No post-install config required. QEMU/Hypervisor.framework works out of the box.

### Windows

Only needed when VirtualBox is the driver:
```
multipass set local.driver=virtualbox
```

---

## Pre-flight Check in `polis run`

Before calling `multipass launch`, the CLI must verify:

1. `multipass` is on PATH → else: print install instructions for detected OS
2. Multipass version ≥ 1.16.0 → else: "Update multipass: {platform-specific command}"
3. Linux + snap: `removable-media` connected → else: "Run: sudo snap connect multipass:removable-media"
4. Sufficient disk space (≥ 10 GB free) → else: "Free up disk space before continuing"

All checks must emit actionable, platform-specific error messages. No check
should silently fail.

---

## Version Check Logic

Parse `multipass version` output:

```
multipass   1.16.1
multipassd  1.16.1
```

Extract the client version (first line), split on whitespace, parse semver.
Minimum required: `1.16.0`.

---

## Files to Create / Modify

| File | Change |
|---|---|
| `scripts/install.sh` | Add multipass auto-install for Linux + macOS; add version check; add socket group check |
| `scripts/install.ps1` | New — Windows installer script |
| `cli/src/commands/run.rs` | Add `check_prerequisites()` before `create_workspace()`; platform-specific error hints |
| `cli/src/commands/doctor.rs` | Add `PrerequisiteChecks` struct; surface multipass version + hypervisor + removable-media |

---

## Error Message Standards

Every error must include:
1. What is wrong (one sentence)
2. The exact command to fix it
3. A docs link if the fix requires multiple steps

Example:
```
Error: Multipass cannot read the workspace image (AppArmor denied).
Fix:   sudo snap connect multipass:removable-media
```

---

## Out of Scope

- ARM64 polis workspace image: not yet built; ARM64 Linux users get a clear
  "not yet supported" message rather than a silent failure
- Non-snap Linux multipass installs (e.g. built from source): not supported;
  users must ensure `multipass` ≥ 1.16.0 is on PATH themselves
- macOS VirtualBox driver: not used; QEMU is the default and only supported driver
