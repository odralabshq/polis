# install-tools-windows.ps1 — Install all prerequisites for building Polis on Windows
# Requires: PowerShell 5.1+, winget, and an internet connection.
# Run from repo root as a normal user; will UAC-elevate only where needed.
#
# Installs:
#   just         — task runner
#   Docker Desktop   — container builds
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

# ─── PATH fixup for just ────────────────────────────────────────────────────
# winget installs just to its packages dir but doesn't always add it to PATH
if (-not (Test-Command just)) {
    $justPkg = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Filter "Casey.Just*" -Directory -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($justPkg) {
        $justDir = $justPkg.FullName
        $justExe = Get-ChildItem $justDir -Filter "just.exe" -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($justExe) {
            $justBinDir = $justExe.DirectoryName
            $userPath = [System.Environment]::GetEnvironmentVariable("PATH", "User")
            if ($userPath -notlike "*$justBinDir*") {
                [System.Environment]::SetEnvironmentVariable("PATH", "$userPath;$justBinDir", "User")
                $env:PATH += ";$justBinDir"
                Write-Ok "Added just to user PATH: $justBinDir"
            }
        }
    }
}

# ─── Summary ────────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "══════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host " Install complete!" -ForegroundColor Cyan
Write-Host "══════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""
Write-Host " Next steps:"
Write-Host "  1. Close and reopen your terminal (so PATH changes take effect)"
Write-Host "  2. Verify: just --version"
Write-Host "  3. Start Docker Desktop if not already running"
Write-Host "  4. Run: just build-windows"
Write-Host ""
if (-not (Test-Command just)) {
    Write-Host " NOTE: If 'just' is still not found after restarting your terminal," -ForegroundColor Yellow
    Write-Host "       install it manually: winget install Casey.Just" -ForegroundColor Yellow
    Write-Host "       Then add its install location to your PATH." -ForegroundColor Yellow
}
