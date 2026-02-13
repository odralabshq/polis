# TPROXY Workarounds for WSL2 "Bridge Reaper"

This document outlines three architectural workarounds to enable TPROXY (Transparent Proxy) functionality in WSL2 environments where the default bridge networking causes packet drops due to `rp_filter` and Hyper-V anti-spoofing logic.

## Option A: WSL2 Mirrored Networking (Recommended for Windows 11)

**Description**:
Switches WSL2 to "Mirrored Mode", removing the NAT layer between Windows and WSL2. The Linux VM shares the Windows IP identity directly, bypassing the Hyper-V switch packet validation logic that drops TPROXY packets.

**Prerequisites**:

- Windows 11 22H2 or higher.
- WSL version 2.0.0 or higher.

**Pros**:

- Bypasses Hyper-V switch packet validation.
- Eliminates double-NAT issues.
- Simplifies network topology.

**Cons**:

- Experimental feature; may have stability issues (e.g., stalled TCP connections).
- Requires newer Windows versions.

**Implementation**:

1. Create or edit `%UserProfile%/.wslconfig` on your Windows host.
2. Add the following configuration:

    ```ini
    [wsl2]
    networkingMode=mirrored
    dnsTunneling=true
    firewall=true
    autoProxy=true
    ```

    **Configuration Breakdown**:
    - **`networkingMode=mirrored`**: Overhauls the network architecture so WSL2 shares the Windows host's IP address and network namespace. This removes the NAT (Network Address Translation) layer entirely. TPROXY packets are no longer "spoofed" from a private subnet; they appear as local traffic, bypassing the Hyper-V switch's anti-spoofing drops.
    - **`dnsTunneling=true`**: Changes how WSL2 resolves DNS. Instead of sending packets to a virtual router, it "tunnels" requests directly to the Windows DNS resolver. This improves compatibility with VPNs and corporate firewalls that might block the virtual router.
    - **`firewall=true`**: Enforces Windows Defender Firewall rules on WSL2 traffic. By default in mirrored mode, WSL2 might bypass some host firewall rules; this ensures your security policy (allow/block) applies to the Linux VM too.
    - **`autoProxy=true`**: Automatically syncs HTTP/HTTPS proxy settings from Windows (Settings -> Network & Internet -> Proxy) into the WSL2 VM. If you use a corporate proxy on Windows, this makes it work in Linux without manual `export http_proxy=...`.

3. Restart WSL: `wsl --shutdown`

---

## Option B: IPvlan Docker Network

**Description**:
Replaces the standard Docker `bridge` network with `ipvlan` (L2 or L3 mode). This driver is lighter and often bypasses the standard Linux bridge `rp_filter` checks because it shares the master interface's MAC address (L3) or stacks on it (L2).

**Pros**:

- Very high performance.
- Bypasses standard Linux bridge logic.
- Often resolves `rp_filter` drops without tuning.

**Cons**:

- **L2 Mode**: Requires promiscuous mode or MAC spoofing enabled on the Hyper-V vSwitch.
- **L3 Mode**: Containers cannot communicate with the host IP directly (requires routing tricks).
- More complex to configure than standard bridges.

**Implementation**:

1. Enable MAC Spoofing on Windows Host (PowerShell Admin):

    ```powershell
    Get-VMNetworkAdapter -Name "WSL" | Set-VMNetworkAdapter -MacAddressSpoofing On
    ```

2. Create an IPvlan network in `docker-compose.yml`:

    ```yaml
    networks:
      jail:
        driver: ipvlan
        driver_opts:
          parent: eth0
          ipvlan_mode: l2
        ipam:
          config:
            - subnet: 172.20.0.0/24
    ```

---

## Option C: Aggressive Tuning (rp_filter & MTU)

**Description**:
Addresses packet drops caused by strict `rp_filter` enforcement on the **WSL2 VM host** (outside the container) and packet fragmentation due to MTU (Maximum Transmission Unit) mismatches between Docker and Hyper-V.

**Pros**:

- No architectural changes (uses standard bridge).
- Works on older Windows/WSL versions.

**Cons**:

- "Fragile" - updates to Docker or WSL can reset these values.
- Requires modifying sysctls on the WSL2 VM host, not just inside containers.

**Implementation**:

1. **Disable Host-Side rp_filter**:
    Run this *inside* WSL2 (but outside Docker) in your shell profile or startup script:

    ```bash
    sudo sysctl -w net.ipv4.conf.all.rp_filter=0
    sudo sysctl -w net.ipv4.conf.eth0.rp_filter=0
    sudo sysctl -w net.ipv4.conf.docker0.rp_filter=0
    ```

2. **Lower Docker MTU**:
    Edit `/etc/docker/daemon.json` inside WSL2:

    ```json
    {
      "mtu": 1400
    }
    ```

3. Restart Docker service: `sudo service docker restart`
