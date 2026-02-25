# install-tools-windows.ps1 — Install all prerequisites for building Polis on Windows
# Requires: PowerShell 5.1+, winget, and an internet connection.
# Run from repo root as a normal user; will UAC-elevate only where needed.
#
# Installs:
#   just         — task runner
#   Docker Desktop   — container builds
#   Packer       — VM image building
#   Windows ADK (Deployment Tools)  — oscdimg for Packer's cd_content ISO creation
#   Hyper-V      — VM hypervisor (via Windows optional feature)
#   shellcheck   — shell script linting
#   hadolint     — Dockerfile linting
#   Rust toolchain   — CLI + toolbox service builds
#   Git          — if not already present
#   zipsign      — artifact signing

$ErrorActionPreference = "Stop"

function Write-Step($msg) { Write-Host "`n==> $msg" -ForegroundColor Cyan }
function Write-Ok($msg) { Write-Host "  ✓ $msg"  -ForegroundColor Green }
function Write-Skip($msg) { Write-Host "  – $msg (already installed)" -ForegroundColor DarkGray }

function Install-WinGet {
    param([string]$Id, [string]$Name, [string[]]$ExtraArgs = @())
    $installed = winget list --id $Id 2>$null | Select-String $Id
    if ($installed) { Write-Skip $Name; return }
    Write-Host "  Installing $Name..."
    $args = @("install", "--id", $Id, "--silent",
        "--accept-package-agreements", "--accept-source-agreements") + $ExtraArgs
    winget @args
    if ($LASTEXITCODE -ne 0) { throw "winget install $Id failed" }
    Write-Ok $Name
}

function Test-Command($cmd) { $null -ne (Get-Command $cmd -ErrorAction SilentlyContinue) }

# ─── Git ────────────────────────────────────────────────────────────────────
Write-Step "Git"
if (Test-Command git) { Write-Skip "Git" }
else { Install-WinGet "Git.Git" "Git" }

# ─── just ───────────────────────────────────────────────────────────────────
Write-Step "just (task runner)"
if (Test-Command just) { Write-Skip "just" }
else { Install-WinGet "Casey.Just" "just" }

# ─── Rust toolchain ─────────────────────────────────────────────────────────
Write-Step "Rust toolchain (rustup)"
if (Test-Command rustup) {
    Write-Skip "rustup"
    rustup update stable
}
else {
    Write-Host "  Installing rustup..."
    $tmp = Join-Path $env:TEMP "rustup-init.exe"
    Invoke-WebRequest "https://win.rustup.rs/x86_64" -OutFile $tmp
    & $tmp -y --default-toolchain stable
    Remove-Item $tmp
    # Add cargo to PATH for the rest of this session
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    Write-Ok "Rust / cargo"
}

# ─── Docker Desktop ─────────────────────────────────────────────────────────
Write-Step "Docker Desktop"
if (Test-Command docker) { Write-Skip "Docker Desktop" }
else {
    Install-WinGet "Docker.DockerDesktop" "Docker Desktop"
    Write-Host "  NOTE: Start Docker Desktop once manually to complete first-run setup."
}

# ─── Packer ─────────────────────────────────────────────────────────────────
Write-Step "Packer"
if (Test-Command packer) { Write-Skip "Packer" }
else { Install-WinGet "HashiCorp.Packer" "Packer" }

# ─── Windows ADK (Deployment Tools — includes oscdimg) ──────────────────────
Write-Step "Windows ADK — Deployment Tools (oscdimg)"
$oscdimg = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10" -Recurse -Filter "oscdimg.exe" `
    -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match "amd64" } | Select-Object -First 1
if ($oscdimg) {
    Write-Skip "Windows ADK / oscdimg ($($oscdimg.FullName))"
}
else {
    Install-WinGet "Microsoft.WindowsADK" "Windows ADK" `
    @("--override", "/quiet /features OptionId.DeploymentTools")
    # Re-discover after install
    $oscdimg = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10" -Recurse -Filter "oscdimg.exe" `
        -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match "amd64" } | Select-Object -First 1
    Write-Ok "oscdimg at $($oscdimg.FullName)"
}

# Add oscdimg to user PATH permanently (no elevation needed for user PATH)
if ($oscdimg) {
    $userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($userPath -notlike "*Oscdimg*") {
        [Environment]::SetEnvironmentVariable("PATH", "$userPath;$($oscdimg.DirectoryName)", "User")
        $env:PATH = "$($oscdimg.DirectoryName);$env:PATH"
        Write-Ok "oscdimg added to user PATH"
    }
}

# ─── Hyper-V ────────────────────────────────────────────────────────────────
Write-Step "Hyper-V Windows Feature"
$hvState = (Get-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All -ErrorAction SilentlyContinue).State
if ($hvState -eq "Enabled") {
    Write-Skip "Hyper-V"
}
else {
    Write-Host "  Enabling Hyper-V (requires elevation + reboot)..."
    Start-Process powershell -Verb RunAs -Wait -ArgumentList `
        "-NoProfile -Command Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All -NoRestart"
    Write-Ok "Hyper-V enabled (reboot required to take effect)"
}

# ─── Hyper-V Administrators group ───────────────────────────────────────────
Write-Step "Hyper-V Administrators group membership"
$hvAdmins = (Get-LocalGroupMember "Hyper-V Administrators" -ErrorAction SilentlyContinue).Name
if ($hvAdmins -contains "$env:COMPUTERNAME\$env:USERNAME" -or $hvAdmins -contains "$env:USERDOMAIN\$env:USERNAME") {
    Write-Skip "Already a Hyper-V Administrator"
}
else {
    Write-Host "  Adding $env:USERNAME to Hyper-V Administrators (requires elevation)..."
    Start-Process powershell -Verb RunAs -Wait -ArgumentList `
        "-NoProfile -Command Add-LocalGroupMember -Group 'Hyper-V Administrators' -Member '$env:USERNAME'"
    Write-Ok "Added to Hyper-V Administrators (log out and back in to apply)"
}

# ─── Packer HTTP firewall rule ───────────────────────────────────────────────
Write-Step "Packer HTTP server firewall rule (ports 8000-9000)"
$existing = Get-NetFirewallRule -DisplayName "Packer_http_server" -ErrorAction SilentlyContinue
if ($existing) {
    Write-Skip "Packer_http_server firewall rule"
} else {
    $isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if ($isAdmin) {
        New-NetFirewallRule -DisplayName "Packer_http_server" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 8000-9000 | Out-Null
        Write-Ok "Firewall rule created"
    } else {
        Write-Host "  Creating firewall rule (requires elevation)..."
        Start-Process powershell -Verb RunAs -Wait -ArgumentList `
            "-NoProfile -Command New-NetFirewallRule -DisplayName 'Packer_http_server' -Direction Inbound -Action Allow -Protocol TCP -LocalPort 8000-9000"
        Write-Ok "Firewall rule created"
    }
}

# ─── shellcheck ─────────────────────────────────────────────────────────────
Write-Step "shellcheck"
if (Test-Command shellcheck) { Write-Skip "shellcheck" }
else { Install-WinGet "koalaman.shellcheck" "shellcheck" }

# ─── hadolint ───────────────────────────────────────────────────────────────
Write-Step "hadolint"
if (Test-Command hadolint) { Write-Skip "hadolint" }
else {
    $target = "$env:LOCALAPPDATA\Microsoft\WinGet\Links\hadolint.exe"
    $url = "https://github.com/hadolint/hadolint/releases/download/v2.12.0/hadolint-Windows-x86_64.exe"
    Write-Host "  Downloading hadolint..."
    New-Item -ItemType Directory -Path (Split-Path $target) -Force | Out-Null
    Invoke-WebRequest $url -OutFile $target
    Write-Ok "hadolint installed at $target"
}

# ─── zipsign ────────────────────────────────────────────────────────────────
Write-Step "zipsign"
if (Test-Command zipsign) { Write-Skip "zipsign" }
else {
    $cargoZipsign = "$env:USERPROFILE\.cargo\bin\zipsign.exe"
    if (Test-Path $cargoZipsign) {
        Write-Skip "zipsign (via cargo)"
    }
    elseif (Test-Command cargo) {
        Write-Host "  Installing zipsign via cargo..."
        cargo install zipsign
        Write-Ok "zipsign"
    }
    else {
        Write-Warning "cargo not found — install Rust first, then run: cargo install zipsign"
    }
}

# ─── Summary ────────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "══════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host " Install complete! Verify with: just install-tools-windows --check" -ForegroundColor Cyan
Write-Host "══════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""
Write-Host " Next steps:"
Write-Host "  1. Start Docker Desktop if not already running"
Write-Host "  2. Log out and back in (for Hyper-V Administrators group)"
Write-Host "  3. Reboot if Hyper-V was just enabled"
Write-Host "  4. Run: just build-windows"
