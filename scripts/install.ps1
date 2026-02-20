# Polis Installer for Windows
# Usage: irm https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.ps1 | iex
# Or:    .\install.ps1 [-Version v0.3.0] [-Image path\to\image.qcow2]
[CmdletBinding()]
param(
    [string]$Version = "latest",
    [string]$Image   = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoOwner = "OdraLabsHQ"
$RepoName  = "polis"
$MultipassMin = [version]"1.16.0"
$MultipassVersion = "1.16.1"
$InstallDir = Join-Path $env:USERPROFILE ".polis"

function Write-Info  { param($msg) Write-Host "[INFO]  $msg" -ForegroundColor Cyan }
function Write-Ok    { param($msg) Write-Host "[OK]    $msg" -ForegroundColor Green }
function Write-Warn  { param($msg) Write-Host "[WARN]  $msg" -ForegroundColor Yellow }
function Write-Err   { param($msg) Write-Host "[ERROR] $msg" -ForegroundColor Red }

# ── Multipass ─────────────────────────────────────────────────────────────────

function Test-HyperV {
    try {
        $f = Get-WindowsOptionalFeature -Online -FeatureName "Microsoft-Hyper-V-All" -ErrorAction SilentlyContinue
        return ($null -ne $f -and $f.State -eq "Enabled")
    } catch { return $false }
}

function Test-VirtualBox {
    return Test-Path "C:\Program Files\Oracle\VirtualBox\VBoxManage.exe"
}

function Test-WSL2 {
    try {
        $out = wsl --status 2>$null
        return ($LASTEXITCODE -eq 0 -and ($out -match "WSL 2"))
    } catch { return $false }
}

function Install-Multipass {
    $msi = Join-Path $env:TEMP "multipass-${MultipassVersion}+win-Windows.msi"
    $url = "https://github.com/canonical/multipass/releases/download/v${MultipassVersion}/multipass-${MultipassVersion}+win-Windows.msi"
    Write-Info "Downloading Multipass ${MultipassVersion}..."
    Invoke-WebRequest -Uri $url -OutFile $msi -UseBasicParsing
    Write-Info "Installing Multipass (this may take a minute)..."
    $proc = Start-Process msiexec -ArgumentList "/i `"$msi`" /quiet /norestart" -Wait -PassThru
    Remove-Item $msi -Force -ErrorAction SilentlyContinue
    if ($proc.ExitCode -ne 0) {
        Write-Err "Multipass installer exited with code $($proc.ExitCode)."
        exit 1
    }
    # Refresh PATH so multipass is available in this session
    $env:PATH = [System.Environment]::GetEnvironmentVariable("PATH", "Machine") + ";" +
                [System.Environment]::GetEnvironmentVariable("PATH", "User")
}

function Assert-Multipass {
    $mp = Get-Command multipass -ErrorAction SilentlyContinue
    if (-not $mp) {
        Write-Info "Multipass not found — installing..."

        if (Test-HyperV) {
            Install-Multipass
        } elseif (Test-VirtualBox) {
            Install-Multipass
            Write-Info "Configuring Multipass to use VirtualBox driver..."
            & multipass set local.driver=virtualbox
            if (Test-WSL2) {
                Write-Warn "WSL2 is active. VirtualBox and WSL2 can conflict."
                Write-Warn "Consider enabling Hyper-V or disabling WSL2 for best results."
            }
        } else {
            Write-Err "Hyper-V is not available and VirtualBox is not installed."
            Write-Host "  Option 1: Enable Hyper-V (Windows Pro/Enterprise):"
            Write-Host "    Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All"
            Write-Host "  Option 2: Install VirtualBox first:"
            Write-Host "    https://www.virtualbox.org/wiki/Downloads"
            Write-Host "  Then re-run: .\install.ps1"
            exit 1
        }
    }

    # Version check
    $verLine = (& multipass version 2>$null | Select-Object -First 1) -replace '\s+', ' '
    $verStr  = ($verLine -split ' ')[1]
    if (-not $verStr) {
        Write-Warn "Could not determine Multipass version — proceeding anyway."
        return
    }
    try {
        $installed = [version]$verStr
        if ($installed -lt $MultipassMin) {
            Write-Err "Multipass $verStr is too old (need >= $MultipassMin)."
            Write-Host "  Update: download the latest installer from https://multipass.run/install"
            exit 1
        }
        Write-Ok "Multipass $verStr OK"
    } catch {
        Write-Warn "Could not parse Multipass version '$verStr' — proceeding anyway."
    }
}

# ── Polis binary ──────────────────────────────────────────────────────────────

function Resolve-Version {
    if ($Version -ne "latest") { return $Version }
    $api = "https://api.github.com/repos/${RepoOwner}/${RepoName}/releases/latest"
    try {
        $resp = Invoke-RestMethod -Uri $api -UseBasicParsing
        return $resp.tag_name
    } catch {
        Write-Err "Failed to resolve latest version from GitHub API."
        Write-Host "  Use: .\install.ps1 -Version v0.3.0"
        exit 1
    }
}

function Install-Polis {
    param([string]$Tag)
    $binDir = Join-Path $InstallDir "bin"
    New-Item -ItemType Directory -Force -Path $binDir | Out-Null

    $base = "https://github.com/${RepoOwner}/${RepoName}/releases/download/${Tag}"
    $exe  = Join-Path $binDir "polis.exe"
    $sha  = Join-Path $env:TEMP "polis.sha256"

    Write-Info "Downloading polis $Tag..."
    Invoke-WebRequest -Uri "$base/polis-windows-amd64.exe"        -OutFile $exe -UseBasicParsing
    Invoke-WebRequest -Uri "$base/polis-windows-amd64.exe.sha256" -OutFile $sha -UseBasicParsing

    Write-Info "Verifying SHA256..."
    $expected = (Get-Content $sha -Raw).Trim().Split()[0]
    $actual   = (Get-FileHash $exe -Algorithm SHA256).Hash.ToLower()
    Remove-Item $sha -Force -ErrorAction SilentlyContinue
    if ($actual -ne $expected.ToLower()) {
        Write-Err "SHA256 mismatch!"
        Write-Host "  Expected: $expected"
        Write-Host "  Actual:   $actual"
        Remove-Item $exe -Force -ErrorAction SilentlyContinue
        exit 1
    }
    Write-Ok "SHA256 verified: $expected"
}

function Add-ToUserPath {
    $binDir  = Join-Path $InstallDir "bin"
    $current = [System.Environment]::GetEnvironmentVariable("PATH", "User")
    if ($current -notlike "*$binDir*") {
        [System.Environment]::SetEnvironmentVariable("PATH", "$current;$binDir", "User")
        $env:PATH += ";$binDir"
        Write-Ok "Added $binDir to user PATH"
    }
}

function Invoke-PolisInit {
    $polis = Join-Path $InstallDir "bin\polis.exe"
    Write-Info "Acquiring workspace image (~3.2 GB)..."
    $initArgs = if ($Image) { @("init", "--image", $Image) } else { @("init") }
    try {
        & $polis @initArgs
    } catch {
        Write-Warn "Image download failed. Run 'polis init' to retry."
    }
}

# ── Main ──────────────────────────────────────────────────────────────────────

Write-Host ""
Write-Host "╔═══════════════════════════════════════════════════════════════╗"
Write-Host "║                    Polis Installer                            ║"
Write-Host "╚═══════════════════════════════════════════════════════════════╝"
Write-Host ""

Assert-Multipass
$tag = Resolve-Version
Write-Info "Installing Polis $tag"
Install-Polis -Tag $tag
Add-ToUserPath
Invoke-PolisInit

Write-Host ""
Write-Ok "Polis installed successfully!"
Write-Host ""
Write-Host "Get started:"
Write-Host "  polis run          # Create workspace"
Write-Host "  polis run claude   # Create workspace with Claude agent"
Write-Host ""
