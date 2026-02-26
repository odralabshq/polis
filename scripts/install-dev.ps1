# Polis Dev Installer for Windows — installs from local build artifacts
# Usage: .\scripts\install-dev.ps1 [-RepoDir C:\path\to\polis]
[CmdletBinding()]
param(
    [string]$RepoDir = (Split-Path $PSScriptRoot -Parent)
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$MultipassMin = [version]"1.16.0"
$InstallDir   = Join-Path $env:USERPROFILE ".polis"

function Write-Info { param($msg) Write-Host "[INFO]  $msg" -ForegroundColor Cyan }
function Write-Ok   { param($msg) Write-Host "[OK]    $msg" -ForegroundColor Green }
function Write-Warn { param($msg) Write-Host "[WARN]  $msg" -ForegroundColor Yellow }
function Write-Err  { param($msg) Write-Host "[ERROR] $msg" -ForegroundColor Red }

# ── Multipass check ───────────────────────────────────────────────────────────

function Assert-Multipass {
    if (-not (Get-Command multipass -ErrorAction SilentlyContinue)) {
        Write-Err "Multipass is required but not installed."
        Write-Host "  Install: https://multipass.run/install"
        exit 1
    }

    $verLine = (& multipass version 2>$null | Select-Object -First 1) -replace '\s+', ' '
    $verStr  = ($verLine -split ' ')[1]
    if ($verStr) {
        # Strip platform suffixes like +win, +mac so [version] can parse it
        $verClean = $verStr -replace '\+.*', ''
        try {
            $installed = [version]$verClean
            if ($installed -lt $MultipassMin) {
                Write-Err "Multipass $verStr is too old (need >= $MultipassMin)."
                Write-Host "  Update: https://multipass.run/install"
                exit 1
            }
            Write-Ok "Multipass $verStr OK"
        } catch {
            Write-Warn "Could not parse Multipass version '$verStr' — proceeding anyway."
        }
    }
}

# ── CLI install ───────────────────────────────────────────────────────────────

function Install-Cli {
    $bin = Join-Path $RepoDir "cli\target\release\polis.exe"
    if (-not (Test-Path $bin)) {
        Write-Err "CLI binary not found at $bin"
        Write-Host "  Build it first: cd $RepoDir; just build-windows"
        exit 1
    }
    $dest = Join-Path $InstallDir "bin"
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    Copy-Item $bin (Join-Path $dest "polis.exe") -Force
    Write-Ok "Installed CLI from $bin"
}

# ── PATH ──────────────────────────────────────────────────────────────────────

function Add-ToUserPath {
    $binDir  = Join-Path $InstallDir "bin"
    $current = [System.Environment]::GetEnvironmentVariable("PATH", "User")
    if ($current -notlike "*$binDir*") {
        [System.Environment]::SetEnvironmentVariable("PATH", "$current;$binDir", "User")
        $env:PATH += ";$binDir"
        Write-Ok "Added $binDir to user PATH"
    }
}

# ── VM init + image loading ───────────────────────────────────────────────────

function Invoke-PolisInit {
    $imagesTar = Join-Path $RepoDir ".build\polis-images.tar.zst"
    if (-not (Test-Path $imagesTar)) {
        Write-Err "Docker images tarball not found: $imagesTar"
        Write-Host "  Build it first: cd $RepoDir; just build-windows"
        exit 1
    }

    $polis = Join-Path $InstallDir "bin\polis.exe"

    # Remove existing VM for a clean install
    $vmExists = $false
    try {
        $ErrorActionPreference = "Continue"
        $null = & multipass info polis 2>&1
        if ($LASTEXITCODE -eq 0) { $vmExists = $true }
        $ErrorActionPreference = "Stop"
    } catch {
        $ErrorActionPreference = "Stop"
    }

    if ($vmExists) {
        Write-Warn "An existing polis VM was found."
        $confirm = Read-Host "Remove it and start fresh? [y/N]"
        if ($confirm -eq 'y') {
            Write-Info "Removing existing polis VM..."
            & multipass delete polis
            & multipass purge
        } else {
            Write-Info "Keeping existing VM."
        }
    }
    Remove-Item (Join-Path $InstallDir "state.json") -ErrorAction SilentlyContinue

    Write-Info "Running: polis start --dev"
    $ErrorActionPreference = "Continue"
    & $polis start --dev
    $startExitCode = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($startExitCode -ne 0) {
        Write-Err "polis start --dev failed."
        exit 1
    }

    Write-Info "Loading Docker images into VM..."
    & multipass transfer $imagesTar polis:/tmp/polis-images.tar.zst
    if ($LASTEXITCODE -ne 0) { Write-Err "Failed to transfer images tarball to VM"; exit 1 }

    & multipass exec polis -- bash -c 'zstd -d /tmp/polis-images.tar.zst --stdout | docker load && rm -f /tmp/polis-images.tar.zst'
    if ($LASTEXITCODE -ne 0) { Write-Err "Failed to load Docker images in VM"; exit 1 }
    Write-Ok "Docker images loaded"

    # Tag loaded images with the CLI version
    $cliVersion = (& $polis --version 2>&1) -replace '^polis\s+', ''
    $tag = "v$cliVersion"
    Write-Info "Tagging images as $tag..."
    $tagScript = 'docker images --format ''{{.Repository}}:{{.Tag}}'' | grep '':latest$'' | while read -r img; do base=${img%%:*}; docker tag $img ${base}:' + $tag + '; done'
    & multipass exec polis -- bash -c $tagScript

    # Pull go-httpbin (small third-party image not built locally)
    & multipass exec polis -- docker pull mccutchen/go-httpbin 2>$null

    # Fix ownership of key files for container uid 65532
    & multipass exec polis -- sudo chown 65532:65532 /opt/polis/certs/valkey/server.key /opt/polis/certs/valkey/client.key /opt/polis/certs/toolbox/toolbox.key 2>$null
    & multipass exec polis -- sudo chown 65532:65532 /opt/polis/certs/ca/ca.key 2>$null

    Write-Info "Starting services..."
    & multipass exec polis -- bash -c 'cd /opt/polis && docker compose --env-file .env up -d --remove-orphans'
    if ($LASTEXITCODE -ne 0) { Write-Err "Failed to start services"; exit 1 }
    Write-Ok "Services started"
}

# ── Main ──────────────────────────────────────────────────────────────────────

Write-Host ""
Write-Host "╔═══════════════════════════════════════════════════════════════╗"
Write-Host "║                 Polis Dev Installer                           ║"
Write-Host "╚═══════════════════════════════════════════════════════════════╝"
Write-Host ""
Write-Info "Repo:        $RepoDir"
Write-Info "Install dir: $InstallDir"
Write-Host ""

Assert-Multipass
Install-Cli
Add-ToUserPath
Invoke-PolisInit

Write-Host ""
Write-Ok "Polis (dev build) installed successfully!"
Write-Host ""
