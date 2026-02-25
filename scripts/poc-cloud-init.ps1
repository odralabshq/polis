# =============================================================================
# POC: Cloud-Init User Journey Simulation (Windows / PowerShell)
# =============================================================================
# Simulates the FULL user journey that `polis start` will perform after the
# cloud-init migration. Run this from the repo root.
#
# What it does:
#   Phase 1 - Launch VM with cloud-init (Docker, Sysbox, hardening)
#   Phase 2 - Bundle config on host -> transfer into VM -> generate certs
#             -> pull Docker images -> start services -> health check
#
# Usage:
#   .\scripts\poc-cloud-init.ps1
#
# Environment:
#   $env:POLIS_IMAGE_VERSION  - image tag for docker compose (default: latest)
#   $env:VM_NAME              - multipass VM name (default: polis-test)
#   $env:VM_CPUS              - CPU count (default: 2)
#   $env:VM_MEMORY            - memory (default: 8G)
#   $env:VM_DISK              - disk size (default: 20G)
# =============================================================================
$ErrorActionPreference = "Stop"

# -- Configuration -----------------------------------------------------------
$ImageVersion = if ($env:POLIS_IMAGE_VERSION) { $env:POLIS_IMAGE_VERSION } else { "latest" }
$VmName       = if ($env:VM_NAME)             { $env:VM_NAME }             else { "polis-test" }
$VmCpus       = if ($env:VM_CPUS)             { $env:VM_CPUS }             else { "2" }
$VmMemory     = if ($env:VM_MEMORY)           { $env:VM_MEMORY }           else { "8G" }
$VmDisk       = if ($env:VM_DISK)             { $env:VM_DISK }             else { "20G" }
$VmPolisRoot  = "/opt/polis"
$CloudInit    = "cloud-init.yaml"

# -- Logging helpers ----------------------------------------------------------
function Write-Info  { param($msg) Write-Host "[INFO]  $msg" -ForegroundColor Cyan }
function Write-Ok    { param($msg) Write-Host "[OK]    $msg" -ForegroundColor Green }
function Write-Warn  { param($msg) Write-Host "[WARN]  $msg" -ForegroundColor Yellow }
function Write-Err   { param($msg) Write-Host "[ERROR] $msg" -ForegroundColor Red }
function Write-Step  { param($msg)
    Write-Host ""
    Write-Host ("=" * 54) -ForegroundColor Cyan
    Write-Host "  $msg" -ForegroundColor Cyan
    Write-Host ("=" * 54) -ForegroundColor Cyan
}

# -- Helper: run multipass and throw on failure ------------------------------
function Invoke-Multipass {
    param([string[]]$CmdArgs, [switch]$AllowFailure)
    $output = & multipass @CmdArgs 2>&1
    if ($LASTEXITCODE -ne 0 -and -not $AllowFailure) {
        $msg = ($output | Out-String).Trim()
        throw "multipass $($CmdArgs -join ' ') failed (exit $LASTEXITCODE): $msg"
    }
    return ($output | Out-String).Trim()
}

# -- Helper: run multipass exec with bash -c ---------------------------------
function Invoke-VmBash {
    param([string]$Script, [switch]$AllowFailure)
    $output = & multipass exec $VmName -- bash -c $Script 2>&1
    if ($LASTEXITCODE -ne 0 -and -not $AllowFailure) {
        $msg = ($output | Out-String).Trim()
        throw "VM command failed (exit $LASTEXITCODE): $msg"
    }
    return ($output | Out-String).Trim()
}

# -- Preflight ---------------------------------------------------------------
function Test-Preflight {
    Write-Step "Preflight checks"

    if (-not (Get-Command multipass -ErrorAction SilentlyContinue)) {
        Write-Err "multipass not found. Install: https://multipass.run/install"
        exit 1
    }
    Write-Ok "multipass found"

    if (-not (Test-Path $CloudInit)) {
        Write-Err "cloud-init.yaml not found. Run from repo root."
        exit 1
    }
    Write-Ok "cloud-init.yaml found"

    if (-not (Test-Path "docker-compose.yml")) {
        Write-Err "docker-compose.yml not found. Run from repo root."
        exit 1
    }
    Write-Ok "docker-compose.yml found"

    # Check tar.exe is available (ships with Windows 10+)
    if (-not (Get-Command tar -ErrorAction SilentlyContinue)) {
        Write-Err "tar not found. Windows 10+ should have tar.exe built-in."
        exit 1
    }
    Write-Ok "tar found"

    if (-not (Test-Path ".build/polis-images.tar")) {
        Write-Err ".build/polis-images.tar not found. Run 'just build' first."
        exit 1
    }
    Write-Ok "Docker images tar found"
}

# -- Cleanup existing VM ----------------------------------------------------
function Remove-ExistingVm {
    $null = & multipass info $VmName 2>&1
    if ($LASTEXITCODE -eq 0) {
        Write-Warn "Existing VM '$VmName' found - deleting..."
        & multipass delete $VmName 2>$null
        & multipass purge 2>$null
        Write-Ok "Old VM removed"
    }
}

# -- Phase 1: Launch VM with cloud-init --------------------------------------
function Start-Phase1 {
    Write-Step "Phase 1: Launch VM with cloud-init"
    Write-Info "Launching $VmName ($VmCpus CPUs, $VmMemory RAM, $VmDisk disk)..."
    Write-Info "This installs Docker, Sysbox, and applies hardening. Takes 3-5 minutes."

    & multipass launch 24.04 `
        --name $VmName `
        --cpus $VmCpus `
        --memory $VmMemory `
        --disk $VmDisk `
        --cloud-init $CloudInit `
        --timeout 900
    if ($LASTEXITCODE -ne 0) { throw "multipass launch failed" }
    Write-Ok "VM launched"

    Write-Info "Waiting for cloud-init to finish..."
    & multipass exec $VmName -- cloud-init status --wait 2>$null
    Write-Ok "Cloud-init complete"
}

# -- Phase 2: Bundle and transfer config ------------------------------------
function Start-Phase2 {
    Write-Step "Phase 2: Bundle config and transfer into VM"

    $bundleDir = Join-Path $env:TEMP "polis-bundle-$(Get-Random)"
    New-Item -ItemType Directory -Force -Path $bundleDir | Out-Null

    try {
        Write-Info "Bundling polis config..."

        # docker-compose.yml (strip @sha256 digests)
        $compose = Get-Content "docker-compose.yml" -Raw
        $compose = $compose -replace '@sha256:[a-f0-9]{64}', ''
        Set-Content -Path (Join-Path $bundleDir "docker-compose.yml") -Value $compose -NoNewline

        # .env with pinned versions
        $envContent = @"
POLIS_RESOLVER_VERSION=$ImageVersion
POLIS_CERTGEN_VERSION=$ImageVersion
POLIS_GATE_VERSION=$ImageVersion
POLIS_SENTINEL_VERSION=$ImageVersion
POLIS_SCANNER_VERSION=$ImageVersion
POLIS_WORKSPACE_VERSION=$ImageVersion
POLIS_HOST_INIT_VERSION=$ImageVersion
POLIS_STATE_VERSION=$ImageVersion
POLIS_TOOLBOX_VERSION=$ImageVersion
"@
        Set-Content -Path (Join-Path $bundleDir ".env") -Value $envContent

        # Service configs and scripts
        $services = @("resolver","certgen","gate","sentinel","scanner","state","toolbox","workspace","host-init")
        foreach ($svc in $services) {
            $svcSrc = "services/$svc"
            if (Test-Path "$svcSrc/config") {
                $dest = Join-Path $bundleDir "services/$svc/config"
                New-Item -ItemType Directory -Force -Path $dest | Out-Null
                Copy-Item -Recurse -Force "$svcSrc/config/*" $dest
            }
            if (Test-Path "$svcSrc/scripts") {
                $dest = Join-Path $bundleDir "services/$svc/scripts"
                New-Item -ItemType Directory -Force -Path $dest | Out-Null
                Copy-Item -Recurse -Force "$svcSrc/scripts/*" $dest
            }
        }

        # Setup scripts
        $scriptsDir = Join-Path $bundleDir "scripts"
        New-Item -ItemType Directory -Force -Path $scriptsDir | Out-Null
        Copy-Item "packer/scripts/setup-certs.sh" $scriptsDir
        Copy-Item "scripts/generate-agent.sh" $scriptsDir

        # Polis config
        $configDir = Join-Path $bundleDir "config"
        New-Item -ItemType Directory -Force -Path $configDir | Out-Null
        Copy-Item "config/polis.yaml" $configDir

        # Placeholder directories for certs and secrets
        foreach ($d in @("certs/ca","certs/valkey","certs/toolbox","secrets")) {
            New-Item -ItemType Directory -Force -Path (Join-Path $bundleDir $d) | Out-Null
        }

        # Create config tarball using Windows tar.exe
        $configTar = Join-Path $bundleDir "polis-config.tar.gz"
        & tar -czf $configTar -C $bundleDir `
            docker-compose.yml .env services scripts config certs secrets
        if ($LASTEXITCODE -ne 0) { throw "tar failed to create config bundle" }

        $tarSize = "{0:N1} KB" -f ((Get-Item $configTar).Length / 1KB)
        Write-Ok "Config bundle created ($tarSize)"

        # Bundle agents
        $agentsDir = Join-Path $bundleDir "agents-staging"
        New-Item -ItemType Directory -Force -Path $agentsDir | Out-Null
        $hasAgents = $false

        foreach ($agentDir in (Get-ChildItem "agents" -Directory -ErrorAction SilentlyContinue)) {
            if ($agentDir.Name -eq "_template") { continue }
            if (-not (Test-Path (Join-Path $agentDir.FullName "agent.yaml"))) { continue }
            Copy-Item -Recurse -Force $agentDir.FullName (Join-Path $agentsDir $agentDir.Name)
            $hasAgents = $true
        }

        $agentsTar = Join-Path $bundleDir "polis-agents.tar.gz"
        if ($hasAgents) {
            & tar -czf $agentsTar -C $agentsDir .
            if ($LASTEXITCODE -ne 0) { throw "tar failed to create agents bundle" }
            $agentSize = "{0:N1} KB" -f ((Get-Item $agentsTar).Length / 1KB)
            Write-Ok "Agents bundle created ($agentSize)"
        }

        # Transfer into VM
        Write-Info "Transferring config bundle into VM..."
        & multipass transfer $configTar "${VmName}:/tmp/polis-config.tar.gz"
        if ($LASTEXITCODE -ne 0) { throw "config transfer failed" }
        Write-Ok "Config transferred"

        if ($hasAgents) {
            Write-Info "Transferring agents bundle into VM..."
            & multipass transfer $agentsTar "${VmName}:/tmp/polis-agents.tar.gz"
            if ($LASTEXITCODE -ne 0) { throw "agents transfer failed" }
            Write-Ok "Agents transferred"
        }

        # Extract inside VM
        Write-Info "Extracting config inside VM..."
        Invoke-VmBash "cd $VmPolisRoot && sudo tar -xzf /tmp/polis-config.tar.gz && sudo chown -R ubuntu:ubuntu $VmPolisRoot && find $VmPolisRoot -name '*.sh' -exec chmod +x {} \; && rm -f /tmp/polis-config.tar.gz"
        Write-Ok "Config extracted to $VmPolisRoot"

        if ($hasAgents) {
            Write-Info "Extracting agents inside VM..."
            Invoke-VmBash "mkdir -p $VmPolisRoot/agents && cd $VmPolisRoot/agents && sudo tar -xzf /tmp/polis-agents.tar.gz && sudo chown -R ubuntu:ubuntu $VmPolisRoot/agents && rm -f /tmp/polis-agents.tar.gz"
            Write-Ok "Agents extracted"
        }
    }
    finally {
        Remove-Item -Recurse -Force $bundleDir -ErrorAction SilentlyContinue
    }
}

# -- Phase 3: Generate certs inside VM ---------------------------------------
function Start-Phase3 {
    Write-Step "Phase 3: Generate certificates"
    Write-Info "Running setup-certs.sh inside VM..."

    Invoke-VmBash "cd $VmPolisRoot && chmod +x scripts/setup-certs.sh && chmod +x services/state/scripts/*.sh 2>/dev/null; chmod +x services/toolbox/scripts/*.sh 2>/dev/null; sudo bash scripts/setup-certs.sh"
    Write-Ok "Certificates generated"
}

# -- Phase 4: Load Docker images ---------------------------------------------
function Start-Phase4 {
    Write-Step "Phase 4: Load Docker images"

    $imagesTar = ".build/polis-images.tar"
    if (-not (Test-Path $imagesTar)) {
        Write-Err "$imagesTar not found. Run 'just build' first to build and export Docker images."
        throw "Missing $imagesTar"
    }

    $tarSize = "{0:N0} MB" -f ((Get-Item $imagesTar).Length / 1MB)
    Write-Info "Transferring Docker images into VM ($tarSize)... this may take a minute."
    & multipass transfer $imagesTar "${VmName}:/tmp/polis-images.tar"
    if ($LASTEXITCODE -ne 0) { throw "image transfer failed" }
    Write-Ok "Images transferred"

    Write-Info "Loading images into Docker..."
    Invoke-VmBash "sudo docker load -i /tmp/polis-images.tar && rm -f /tmp/polis-images.tar"
    Write-Ok "Docker images loaded"

    Write-Info "Available images:"
    Invoke-VmBash "docker images --format 'table {{.Repository}}\t{{.Tag}}\t{{.Size}}'" -AllowFailure
}

# -- Phase 5: Start services ------------------------------------------------
function Start-Phase5 {
    Write-Step "Phase 5: Start services"
    Write-Info "Restarting Docker cleanly (stop sysbox + wipe netns state)..."
    Invoke-VmBash "sudo systemctl stop docker.socket docker 2>/dev/null; sudo systemctl stop sysbox sysbox-mgr sysbox-fs 2>/dev/null; sudo rm -f /var/run/docker/netns/*; sudo systemctl start sysbox-mgr sysbox-fs sysbox 2>/dev/null || true; sudo systemctl reset-failed docker 2>/dev/null; sudo systemctl start docker && sleep 5"
    Write-Info "Running docker compose up -d..."

    # Check for agent overlays
    $composeArgs = "-f $VmPolisRoot/docker-compose.yml"
    $overlays = Invoke-VmBash "for f in $VmPolisRoot/agents/*/.generated/compose.agent.yaml; do [ -f `"`$f`" ] && echo `"`$f`"; done" -AllowFailure
    if ($overlays) {
        foreach ($overlay in ($overlays -split "`n" | Where-Object { $_ })) {
            $composeArgs += " -f $overlay"
        }
        Write-Info "Including agent overlay(s)"
    }

    Invoke-VmBash "cd $VmPolisRoot && docker compose $composeArgs up -d --remove-orphans"
    Write-Ok "Services started"
}

# -- Phase 6: Health check --------------------------------------------------
function Start-Phase6 {
    Write-Step "Phase 6: Health check"
    Write-Info "Waiting for services to become healthy (up to 3 minutes)..."

    $maxAttempts = 18
    $allHealthy = $false

    for ($attempt = 1; $attempt -le $maxAttempts; $attempt++) {
        $status = Invoke-VmBash "cd $VmPolisRoot && docker compose ps --format '{{.Name}} {{.Status}}'" -AllowFailure

        if (-not $status) {
            Write-Info "Attempt $attempt/$maxAttempts`: waiting for containers..."
            Start-Sleep -Seconds 10
            continue
        }

        $lines = $status -split "`n" | Where-Object { $_ }
        $total = $lines.Count
        $unhealthy = ($lines | Where-Object { $_ -notmatch '(healthy|running)' }).Count

        if ($unhealthy -eq 0 -and $total -gt 0) {
            $allHealthy = $true
            break
        }

        Write-Info "Attempt $attempt/$maxAttempts`: $total containers, $unhealthy not ready"
        Start-Sleep -Seconds 10
    }

    Write-Host ""
    Write-Info "Container status:"
    Invoke-VmBash "cd $VmPolisRoot && docker compose ps" -AllowFailure
    Write-Host ""

    if ($allHealthy) {
        Write-Ok "All services healthy"
    } else {
        Write-Warn "Some services may still be starting. Check with:"
        Write-Host "  multipass exec $VmName -- bash -c 'cd $VmPolisRoot && docker compose ps'"
        Write-Host "  multipass exec $VmName -- bash -c 'cd $VmPolisRoot && docker compose logs --tail=20'"
    }
}

# -- Verification ------------------------------------------------------------
function Show-Verification {
    Write-Step "Verification"

    Write-Info "Docker version:"
    & multipass exec $VmName -- docker --version

    Write-Info "Docker Compose version:"
    & multipass exec $VmName -- docker compose version

    Write-Info "Sysbox runtime:"
    Invoke-VmBash "docker info 2>/dev/null | grep -A2 Runtimes" -AllowFailure

    Write-Info "Hardening - kernel.dmesg_restrict:"
    & multipass exec $VmName -- sysctl kernel.dmesg_restrict

    Write-Info "Auditd status:"
    & multipass exec $VmName -- systemctl is-active auditd 2>$null

    Write-Info "Polis directory:"
    & multipass exec $VmName -- ls -la $VmPolisRoot/

    Write-Info "Certificates:"
    Invoke-VmBash "ls -la $VmPolisRoot/certs/ca/ $VmPolisRoot/certs/valkey/ $VmPolisRoot/certs/toolbox/ 2>/dev/null" -AllowFailure

    Write-Info "Secrets:"
    Invoke-VmBash "ls $VmPolisRoot/secrets/ 2>/dev/null" -AllowFailure
}

# -- Main --------------------------------------------------------------------
function Invoke-Poc {
    Write-Host ""
    Write-Host "+============================================================+"
    Write-Host "|    Polis Cloud-Init POC - Full User Journey Simulation     |"
    Write-Host "+============================================================+"
    Write-Host ""

    $startTime = Get-Date

    Test-Preflight
    Remove-ExistingVm
    Start-Phase1
    Start-Phase2
    Start-Phase3
    Start-Phase4
    Start-Phase5
    Start-Phase6
    Show-Verification

    $elapsed = (Get-Date) - $startTime
    $minutes = [math]::Floor($elapsed.TotalMinutes)
    $seconds = $elapsed.Seconds

    Write-Host ""
    Write-Host "+============================================================+"
    Write-Host "|                      POC Complete                          |"
    Write-Host "+============================================================+"
    Write-Host "  Total time: ${minutes}m ${seconds}s"
    Write-Host "  VM name:    $VmName"
    Write-Host "  Connect:    multipass shell $VmName"
    Write-Host "  Cleanup:    multipass delete $VmName; multipass purge"
    Write-Host "+============================================================+"
    Write-Host ""
}

try {
    Invoke-Poc
} catch {
    Write-Err $_.Exception.Message
    Write-Host ""
    Write-Host "To debug, check:" -ForegroundColor Yellow
    Write-Host "  multipass exec $VmName -- cloud-init status"
    Write-Host "  multipass exec $VmName -- bash -c 'cat /var/log/cloud-init-output.log | tail -50'"
    Write-Host "  multipass exec $VmName -- bash -c 'cd $VmPolisRoot && docker compose logs --tail=20'"
    exit 1
}
