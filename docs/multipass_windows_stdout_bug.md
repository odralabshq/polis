# Multipass on Windows: stdout Capture Bug with Piped Commands

## Summary

When running `multipass exec <vm> -- <command>` on **Windows**, any command whose
**last stage is a shell pipe** (e.g. `cmd1 | cmd2`) returns **zero bytes of stdout**
to the calling process, even though the command executes correctly inside the VM
and produces the expected output. Exit code is always `0` (success), making the
failure completely invisible.

This bug affects any process that captures Multipass subprocess output
programmatically — PowerShell, Python `subprocess`, Rust `tokio::process`, etc.

---

## Environment

| Component | Version / Detail |
|---|---|
| **Host OS** | Windows 11 / Windows 10 |
| **Multipass** | 1.16.1+win (confirmed), likely affects all Windows versions |
| **VM Image** | Ubuntu 24.04 LTS |
| **Caller** | Rust CLI using `tokio::process::Command` with `Stdio::piped()` |
| **Docker Compose** | v2.x (inside VM) |

---

## Reproduction

### Case 1 — Single command (works correctly)
```powershell
multipass exec polis -- cat /proc/uptime
# Output: "1764.75 1726.69"  ✅ stdout captured correctly
```

### Case 2 — Pipe inside VM (broken)
```powershell
multipass exec polis -- bash -c "cat /proc/uptime | cat"
# Output: ""  ❌ empty stdout, exit code 0
```

### Case 3 — Complex pipe (broken)
```powershell
multipass exec polis -- bash -c "docker compose -f /opt/polis/docker-compose.yml ps --format json | base64 -w 0"
# Output: ""  ❌ empty stdout, exit code 0
```

### Case 4 — Redirect to file, then cat (workaround attempt, still broken)
```powershell
# Step 1: write to file — works
multipass exec polis -- bash -c "cat /proc/uptime > /tmp/out.txt"

# Step 2: cat the file directly — ALSO affected in some contexts
multipass exec polis -- cat /tmp/out.txt
# Output: hangs or returns empty depending on file size  ❌
```

### Case 5 — Confirmed from Rust `tokio::process`
```rust
// stdout.len() == 0 when command involves a pipe
let output = Command::new("multipass")
    .args(["exec", "polis", "--", "bash", "-c", "docker compose ps --format json | base64 -w 0"])
    .stdout(Stdio::piped())
    .output()
    .await?;
println!("stdout_len={}", output.stdout.len()); // prints: stdout_len=0
println!("success={}", output.status.success()); // prints: success=true
```

---

## Affected Commands in Polis CLI

This bug affects every `mp.exec()` call that either:
1. Runs a **shell pipeline** inside the VM, OR
2. Produces **large stdout** (observed truncation even without pipes in some cases)

| Service | Call | Impact |
|---|---|---|
| [workspace_status.rs](file:///e:/odralabs/polis/cli/src/application/services/workspace_status.rs) | `bash -c "cat /proc/uptime && echo --- && docker compose ps --format json"` | Status always shows "starting", all security features disabled |
| [workspace_doctor.rs](file:///e:/odralabs/polis/cli/src/application/services/workspace_doctor.rs) | `docker compose -f ... ps --format json gate` | Gate health check always returns [false](file:///e:/odralabs/polis/cli/src/application/services/vm/lifecycle.rs#423-428) |
| [workspace_doctor.rs](file:///e:/odralabs/polis/cli/src/application/services/workspace_doctor.rs) | `docker exec polis-scanner sh -c "stat ... \| sort \| head"` | Malware DB age always reports `0`, always fails |
| [config_service.rs](file:///e:/odralabs/polis/cli/src/application/services/config_service.rs) | `cat /secrets/valkey_mcp_admin_password.txt` | May fail for large secret files |
| [workspace_repair.rs](file:///e:/odralabs/polis/cli/src/application/services/workspace_repair.rs) | `bash -c "cd /opt/polis && docker compose down"` | Repair result not verifiable |
| [update.rs](file:///e:/odralabs/polis/cli/src/infra/update.rs) | Various `bash -c` docker commands | Update verification potentially unreliable |

---

## Root Cause Analysis

### What Multipass does internally on Windows
Multipass on Windows communicates with the VM over a gRPC-based RPC transport
(`multipass.proto`). The [exec](file:///e:/odralabs/polis/cli/src/application/services/vm/lifecycle.rs#523-529) command opens a streaming RPC call and proxies
the child process's stdin/stdout through this channel.

On Linux/macOS, Multipass uses a UNIX socket for this transport. On Windows, it
uses a named pipe backed by the Windows `multipassd` service.

### Hypothesis A — TTY allocation interferes with pipe detection
When the calling terminal is a TTY (interactive PowerShell/Windows Terminal),
Multipass may allocate a pseudo-TTY inside the VM. When the VM-side command
involves a pipe, the shell may detect the TTY and behave differently (e.g., 
buffering, flushing only on newline, or not flushing at all before exit).

This is similar to the classic Linux issue where `cmd | head -n 1` causes the
first command to receive SIGPIPE from the closed pipe end.

### Hypothesis B — gRPC stream closes before data is flushed
The Multipass Windows client may close the gRPC stream prematurely when the
top-level process ([bash](file:///e:/odralabs/polis/tests/lib/guards.bash)) exits, before the OS has flushed the pipe buffer of
the child process (`base64`, `docker`, etc.) back to [bash](file:///e:/odralabs/polis/tests/lib/guards.bash)'s stdout.

On Linux, process groups and wait() semantics prevent this. The Windows gRPC
stream may not have equivalent guarantees.

### Hypothesis C — Named pipe buffer overflow
Windows named pipes have a fixed buffer (default 4096 bytes). For large outputs,
the pipe may block waiting for the reader on the host side. If the Multipass
gRPC client's reader doesn't drain fast enough, the VM-side process blocks on
write → deadlock → eventual timeout with empty output.

This would explain why the problem is worse with large JSON output (docker compose
ps with many containers) and why intermediate-sized outputs sometimes work.

### Evidence Supporting Multiple Hypotheses
- `exit_ok=true stdout_len=0`: Process thinks it succeeded, host received nothing → H-B or H-C
- Large outputs worse than small outputs → H-C
- Simple `cat file` works, `cat file | cat` doesn't → H-A or H-B (pipe presence triggers it)
- `multipass exec polis -- printf 'hello' | base64 -w 0` works from PowerShell directly,
  but same command via `tokio::process` returns empty → H-A (TTY presence matters)

---

## Workarounds Evaluated

### ✅ Write-to-file + transfer
Write output to a temp file inside the VM, then use `multipass transfer` to pull
it to the host and read it locally.
- **Pro**: Reliable, avoids all stdout issues
- **Con**: Two multipass commands + one file write; requires tmp cleanup

### ✅ Write-to-file + cat (partial)
Write to `/tmp/file.txt`, then `cat /tmp/file.txt` as a separate exec call.
Works for small files, unreliable for large files due to H-C.

### ❌ Base64 pipe (doesn't work)
Pipe everything through `base64 -w 0` inside the VM.
Fails because the pipe itself triggers the bug regardless of encoding.

### ❌ Redirect with [tee](file:///e:/odralabs/polis/cli/src/output/mod.rs#196-214)
`command | tee /tmp/out.txt` — the pipe to [tee](file:///e:/odralabs/polis/cli/src/output/mod.rs#196-214) triggers the bug.

### ⬜ In-VM agent / proxy binary
Deploy a small binary inside the VM that writes structured data to a file
and serves it over a Unix socket. The host CLI connects via `multipass exec polis -- polis-agent status` where `polis-agent` is a simple binary with no pipes in its stdout path.
- **Pro**: Permanently fixes the problem, enables richer introspection, better performance
- **Con**: Additional binary to build, version, and ship in provisioning

### ⬜ SSH directly into VM
Use `multipass info` to get the VM IP, then SSH directly over TCP.
Standard SSH has no stdout capture issues even with pipes.
- **Pro**: Battle-tested, no platform-specific quirks
- **Con**: Requires key management, SSH setup during provisioning, firewall rules

### ⬜ Multipass REST API (if exposed)
Recent Multipass versions expose a REST API at `http://localhost:50051` (or similar).
If it exposes exec properly, this bypasses the Windows CLI entirely.
- **Pro**: Proper API, likely better stdout handling
- **Con**: API is internal/unstable, not officially documented for external use

---

## Known Similar Issues

- GitHub: `multipass exec` long outputs make terminal unresponsive (hanging):
  https://github.com/canonical/multipass/issues/XXXX
- Stack Overflow: `multipass exec` stdout blank when piping on Windows
- Docker for Windows: similar TTY + pipe + named pipe issues in `docker exec`

---

## Search Terms for Research

```
multipass exec windows stdout empty pipe
multipass exec windows zero bytes output
multipass exec piped command no output windows
multipass grpc named pipe stdout buffer windows
"multipass exec" windows "stdout" "0" bytes
multipass exec stdout capture programmatic windows rust python
canonical multipass windows exec stdout pipe issue
```

---

## Impact Assessment for Polis CLI

| Command | Status |
|---|---|
| `polis status` | Broken — shows "starting" and all security inactive |
| `polis doctor` | Partially broken — gate/malware checks always fail |
| `polis update` | Potentially unreliable — verification output not captured |
| `polis start` | Likely OK — uses fire-and-forget exec patterns |
| `polis stop` | Likely OK — same |
| `polis delete` | Likely OK — same |
| `polis connect` | Unknown — depends on SSH key reading path |
| `polis config` | Potentially affected — reads secrets from VM |

---

*Document created: 2026-02-28. Author: Antigravity AI / Polis CLI investigation.*
