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
        try {
            $installed = [version]$verStr
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
        Write-Host "  Build it first: cd $RepoDir\cli; cargo build --release"
        exit 1
    }
    $dest = Join-Path $InstallDir "bin"
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    Copy-Item $bin (Join-Path $dest "polis.exe") -Force
    Write-Ok "Installed CLI from $bin"
}

# ── Image init ────────────────────────────────────────────────────────────────

function Invoke-ImageInit {
    $outputDir = Join-Path $RepoDir "packer\output"
    $image = Get-ChildItem $outputDir -Filter "*.qcow2" -ErrorAction SilentlyContinue |
             Sort-Object Name | Select-Object -Last 1
    if (-not $image) {
        Write-Err "No VM image found in $outputDir"
        Write-Host "  Build it first: just build-vm"
        exit 1
    }
    $sidecar = $image.FullName + ".sha256"
    if (-not (Test-Path $sidecar)) {
        Write-Err "No signed sidecar found at $sidecar"
        Write-Host "  Run: just build-vm  (signing happens automatically)"
        exit 1
    }
    $pubKey = Join-Path $RepoDir ".secrets\polis-release.pub"
    if (-not (Test-Path $pubKey)) {
        Write-Err "Dev public key not found at $pubKey"
        Write-Host "  Run: just build-vm  (keypair is generated automatically)"
        exit 1
    }
    $keyB64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($pubKey))
    $polis  = Join-Path $InstallDir "bin\polis.exe"
    Write-Info "Acquiring workspace image from $($image.FullName)..."
    $env:POLIS_VERIFYING_KEY_B64 = $keyB64
    try {
        & $polis init --image $image.FullName
    } catch {
        Write-Warn "Image init failed. Run manually:"
        Write-Host "  `$env:POLIS_VERIFYING_KEY_B64='$keyB64'; polis init --image $($image.FullName)"
    } finally {
        Remove-Item Env:\POLIS_VERIFYING_KEY_B64 -ErrorAction SilentlyContinue
    }
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
Invoke-ImageInit

Write-Host ""
Write-Ok "Polis (dev build) installed successfully!"
Write-Host ""
Write-Host "Get started:"
Write-Host "  polis run"
Write-Host ""
