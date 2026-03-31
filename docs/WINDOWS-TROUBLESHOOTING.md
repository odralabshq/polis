# Windows Troubleshooting — Polis + Hyper-V + Multipass

Common issues encountered when running Polis on Windows with
Hyper-V and Multipass, with root causes and solutions.

---

## 1. VPN Breaks VM Networking (NordVPN, WireGuard, etc.)

**Symptoms:**
- `polis start` fails with `Timed out waiting for instance launch`
- `multipass list` shows `N/A` for IPv4
- VM is "Running" but unreachable

**Root cause:** VPN tunnel adapters (NordLynx/WireGuard, OpenVPN TAP)
interfere with the Hyper-V Default Switch's ICS DHCP server. The VM
boots but never gets a DHCP lease, so it has no IP address. Multipass
can't SSH in and reports a timeout.

**Solution:**
1. Disconnect VPN completely (check that the tunnel adapter is gone):
   ```powershell
   Get-NetAdapter | Where-Object Status -eq Up | Select-Object Name
   # NordLynx, TAP-*, tun* should NOT appear
   ```
2. Delete the broken VM and start fresh:
   ```powershell
   multipass stop polis --force
   multipass delete polis --purge
   polis start
   ```

**Prevention:** Always disconnect VPN before running `polis start`.
Reconnect after the VM is healthy (`multipass list` shows an IP).

---

## 2. Stale hosts File Entry Misdirects SSH

**Symptoms:**
- `multipass list` shows the VM running with an IP
- `multipass exec polis -- echo test` fails with
  `ssh connection failed: Timeout connecting to polis.mshome.net`
- But `Test-NetConnection <vm-ip> -Port 22` succeeds

**Root cause:** The Windows hosts file
(`C:\Windows\System32\drivers\etc\hosts`) contains a stale entry
mapping `polis.mshome.net` to an old IP from a previous VM. Multipass
resolves the hostname via the OS, hits the dead IP, and times out.
The actual VM is fine on a different IP.

This happens when:
- The Hyper-V Default Switch was reset (reboot), changing the subnet
- A previous debugging session added a manual hosts entry
- The ICS hosts file and the regular hosts file have conflicting entries

**Diagnosis:**
```powershell
# Check what the hosts file says
Select-String "polis" C:\Windows\System32\drivers\etc\hosts

# Find the actual VM IP
arp -a | findstr 172.

# Compare — if they don't match, that's the problem
```

**Solution (requires admin PowerShell):**
```powershell
# Option A: Remove the stale entry entirely
$h = Get-Content C:\Windows\System32\drivers\etc\hosts
$h | Where-Object { $_ -notmatch 'polis' } | Set-Content C:\Windows\System32\drivers\etc\hosts
ipconfig /flushdns

# Option B: Update to the correct IP
# Find the VM IP from arp -a, then:
$h = Get-Content C:\Windows\System32\drivers\etc\hosts
$h -replace '.*polis\.mshome\.net.*', '<correct-ip> polis.mshome.net' | Set-Content C:\Windows\System32\drivers\etc\hosts
ipconfig /flushdns
```

**Prevention:** Don't manually add `polis.mshome.net` to the hosts
file. If you must, remove it before running `polis start` again.

---

## 3. Insufficient RAM (8 GB Required)

**Symptoms:**
- `polis start` fails with `Unable to allocate 8192 MB of RAM`
- Error mentions `Insufficient system resources (0x800705AA)`

**Root cause:** Polis requests 8 GB for the Hyper-V VM. If your
system doesn't have 8 GB of contiguous free RAM, Hyper-V refuses.

**Common RAM consumers on a 32 GB system:**
| Consumer | Typical Usage |
|----------|--------------|
| WSL 2 VM | 2-10 GB (check `.wslconfig` `memory=` setting) |
| Browser (Brave/Chrome) | 2-4 GB |
| VS Code / Kiro | 2-3 GB |
| Docker Desktop | 2-4 GB (not needed — Polis runs its own Docker) |
| Antivirus (Bitdefender) | 0.5-1 GB |

**Solution:**
```powershell
# Check free RAM
$os = Get-CimInstance Win32_OperatingSystem
[math]::Round($os.FreePhysicalMemory/1MB,1)
# Need at least 9-10 GB free (8 GB VM + overhead)

# Free RAM by shutting down WSL (biggest win)
wsl --shutdown

# Quit Docker Desktop (system tray → right-click → Quit)
# Polis runs Docker inside the VM, Docker Desktop is not needed

# Close unnecessary browser tabs

# Reduce WSL memory limit permanently (edit as admin):
# C:\Users\<you>\.wslconfig
# [wsl2]
# memory=4GB
```

**Check WSL memory allocation:**
```powershell
Get-Content "$env:USERPROFILE\.wslconfig" -ErrorAction SilentlyContinue
# If memory=10GB, that's 10 GB reserved even when WSL is idle
```

---

## 4. "instance already exists" After Failed Install

**Symptoms:**
- `polis start` fails with `instance "polis" already exists`
- Previous install attempt timed out, leaving a half-configured VM

**Root cause:** The installer timed out during SSH/config transfer,
but the VM was actually created successfully. The Polis CLI state
file (`~/.polis/state.json`) was deleted by the installer's cleanup,
so the CLI doesn't know the VM exists and tries to create a new one.

**Solution:**
```powershell
# Delete the orphaned VM
multipass stop polis --force
multipass delete polis --purge

# Remove stale CLI state
Remove-Item "$env:USERPROFILE\.polis\state.json" -Force -ErrorAction SilentlyContinue

# Verify clean
multipass list   # Should show "No instances found"

# Reinstall
polis start
# or: irm https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.ps1 | iex
```

---

## 5. Default Switch DHCP Broken After VPN Use

**Symptoms:**
- New VMs never get an IP (even after deleting and recreating)
- `multipass list` always shows `N/A` for IPv4
- `arp -a` on the Default Switch subnet shows no VM entries
- Happens consistently after VPN was used

**Root cause:** The VPN corrupted the Hyper-V Default Switch's
ICS (Internet Connection Sharing) NAT/DHCP tables. The ICS service
provides DHCP for the Default Switch, and VPN tunnel adapters can
break its routing state. This persists across VM deletions.

**Solution — restart ICS service (try first):**
```powershell
# Admin PowerShell
Restart-Service SharedAccess -Force
Restart-Service vmms -Force
```

**Solution — reset Default Switch (guaranteed fix, requires reboot):**
```powershell
# Admin PowerShell
Get-HNSNetwork | Where-Object { $_.Name -like "Default Switch" } | Remove-HNSNetwork
Restart-Computer
# Hyper-V recreates the Default Switch on boot with fresh DHCP state
```

After reboot, verify the Default Switch has a new IP range:
```powershell
Get-NetIPAddress -InterfaceAlias "vEthernet (Default Switch)" -AddressFamily IPv4
# Should show a 172.x.x.1 address
```

---

## Full Recovery Procedure

If nothing else works, this sequence resets everything:

```powershell
# 1. Disconnect VPN
# 2. Shut down WSL
wsl --shutdown

# 3. Quit Docker Desktop (system tray)

# 4. Delete any existing Polis VM
multipass stop polis --force
multipass delete polis --purge

# 5. Remove Polis CLI state
Remove-Item "$env:USERPROFILE\.polis\state.json" -Force -ErrorAction SilentlyContinue

# 6. Remove stale hosts entries (admin PowerShell)
$h = Get-Content C:\Windows\System32\drivers\etc\hosts
$h | Where-Object { $_ -notmatch 'polis' } | Set-Content C:\Windows\System32\drivers\etc\hosts
ipconfig /flushdns

# 7. Reset Default Switch if needed (admin PowerShell + reboot)
Get-HNSNetwork | Where-Object { $_.Name -like "Default Switch" } | Remove-HNSNetwork
Restart-Computer

# 8. After reboot, install fresh
irm https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.ps1 | iex
```
