# =============================================================================
# Polis Installer for Windows
# =============================================================================
# One-line install: irm https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.ps1 | iex
# =============================================================================
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Version          = $env:POLIS_VERSION ?? "0.3.0-preview-6"
$InstallDir       = $env:POLIS_HOME    ?? (Join-Path $env:USERPROFILE ".polis")
$ImageDir         = Join-Path $env:ProgramData "Polis\images"
$RepoOwner        = "OdraLabsHQ"
$RepoName         = "polis"
$MultipassMin     = [version]"1.16.0"
$MultipassVersion = "1.16.1"

function Write-Info { param($msg) Write-Host "[INFO]  $msg" -ForegroundColor Cyan }
function Write-Ok   { param($msg) Write-Host "[OK]    $msg" -ForegroundColor Green }
function Write-Warn { param($msg) Write-Host "[WARN]  $msg" -ForegroundColor Yellow }
function Write-Err  { param($msg) Write-Host "[ERROR] $msg" -ForegroundColor Red }

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
    $env:PATH = [System.Environment]::GetEnvironmentVariable("PATH", "Machine") + ";" +
                [System.Environment]::GetEnvironmentVariable("PATH", "User")
}

function Assert-Multipass {
    if (-not (Get-Command multipass -ErrorAction SilentlyContinue)) {
        Write-Info "Multipass not found — installing..."
        if (Test-HyperV) {
            Install-Multipass
        } elseif (Test-VirtualBox) {
            Install-Multipass
            Write-Info "Configuring Multipass to use VirtualBox driver..."
            & multipass set local.driver=virtualbox
            if (Test-WSL2) {
                Write-Warn "WSL2 is active. VirtualBox and WSL2 can conflict."
            }
        } else {
            Write-Err "Hyper-V is not available and VirtualBox is not installed."
            Write-Host "  Option 1: Enable Hyper-V: Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All"
            Write-Host "  Option 2: Install VirtualBox: https://www.virtualbox.org/wiki/Downloads"
            exit 1
        }
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

# ── CLI ───────────────────────────────────────────────────────────────────────

function Install-Cli {
    $binDir = Join-Path $InstallDir "bin"
    New-Item -ItemType Directory -Force -Path $binDir | Out-Null

    $base = "https://github.com/${RepoOwner}/${RepoName}/releases/download/${Version}"
    $exe  = Join-Path $binDir "polis.exe"
    $sha  = Join-Path $env:TEMP "polis.sha256"

    Write-Info "Downloading polis CLI ${Version}..."
    Invoke-WebRequest -Uri "$base/polis-windows-amd64.exe"        -OutFile $exe -UseBasicParsing
    Invoke-WebRequest -Uri "$base/polis-windows-amd64.exe.sha256" -OutFile $sha -UseBasicParsing

    Write-Info "Verifying CLI SHA256..."
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
    Write-Ok "CLI SHA256 verified: $expected"
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

# ── Image ─────────────────────────────────────────────────────────────────────

function Get-Image {
    $base      = "https://github.com/${RepoOwner}/${RepoName}/releases/download/${Version}"
    $versions  = Invoke-RestMethod -Uri "$base/versions.json" -UseBasicParsing
    $imageName = $versions.vm_image.asset
    $dest      = Join-Path $ImageDir $imageName

    New-Item -ItemType Directory -Force -Path $ImageDir | Out-Null

    Write-Info "Downloading VM image..."
    Invoke-WebRequest -Uri "$base/$imageName" -OutFile $dest -UseBasicParsing

    Write-Info "Verifying image SHA256..."
    $checksums = Invoke-WebRequest -Uri "$base/checksums.sha256" -UseBasicParsing
    $expected  = (($checksums.Content -split "`n" | Where-Object { $_ -match $imageName }) -replace '\s.*', '') | Select-Object -First 1
    $actual    = (Get-FileHash $dest -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $expected.ToLower()) {
        Write-Err "Image SHA256 mismatch!"
        Write-Host "  Expected: $expected"
        Write-Host "  Actual:   $actual"
        Remove-Item $dest -Force -ErrorAction SilentlyContinue
        exit 1
    }
    Write-Ok "Image SHA256 verified: $expected"
    return $dest
}

# ── Init ──────────────────────────────────────────────────────────────────────

function Invoke-PolisInit {
    param([string]$ImagePath)
    $polis = Join-Path $InstallDir "bin\polis.exe"

    $null = & multipass info polis 2>$null
    if ($LASTEXITCODE -eq 0) {
        Write-Warn "An existing polis VM was found."
        $confirm = Read-Host "Remove it and start fresh? [y/N]"
        if ($confirm -eq 'y') {
            Write-Info "Removing existing polis VM..."
            & multipass delete polis
            & multipass purge
        } else {
            Write-Info "Keeping existing VM. Skipping removal."
        }
    }
    Remove-Item (Join-Path $InstallDir "state.json") -Force -ErrorAction SilentlyContinue

    Write-Info "Running: polis start --image $ImagePath"
    & $polis start --image $ImagePath
    if ($LASTEXITCODE -ne 0) {
        Write-Err "polis start failed. Run manually:"
        Write-Host "  polis start --image $ImagePath"
        exit 1
    }
}

# ── Main ──────────────────────────────────────────────────────────────────────

Write-Host ""
Write-Host "╔═══════════════════════════════════════════════════════════════╗"
Write-Host "║                    Polis Installer                            ║"
Write-Host "╚═══════════════════════════════════════════════════════════════╝"
Write-Host ""

Assert-Multipass
Write-Info "Installing Polis ${Version}"
Install-Cli
Add-ToUserPath
$imagePath = Get-Image
Invoke-PolisInit -ImagePath $imagePath

Write-Host ""
Write-Ok "Polis installed successfully!"
Write-Host ""
