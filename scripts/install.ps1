# =============================================================================
# Polis Installer for Windows
# =============================================================================
# One-line install: irm https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.ps1 | iex
# =============================================================================
Set-StrictMode -Version Latest

# Ensure TLS 1.2 for GitHub downloads (PS 5.1 defaults to TLS 1.0)
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Version          = $null  # Resolved in Invoke-PolisInstall via Resolve-Version
$InstallDir       = if ($env:POLIS_HOME)     { $env:POLIS_HOME }     else { Join-Path $env:USERPROFILE ".polis" }
$RepoOwner        = "OdraLabsHQ"
$RepoName         = "polis"
$MultipassMin     = [version]"1.16.0"
$MultipassVersion = "1.16.1"

function Write-Info { param($msg) Write-Host "[INFO]  $msg" -ForegroundColor Cyan }
function Write-Ok   { param($msg) Write-Host "[OK]    $msg" -ForegroundColor Green }
function Write-Warn { param($msg) Write-Host "[WARN]  $msg" -ForegroundColor Yellow }
function Write-Err  { param($msg) Write-Host "[ERROR] $msg" -ForegroundColor Red }

function Show-WindowsNetworkingNote {
    Write-Warn "Windows networking note for Multipass:"
    Write-Host "  - Use a Private network profile on your active adapter."
    Write-Host "  - Turn off VPN during VM creation/startup if networking is unstable."
}

function Confirm-InstallerProceed {
    Write-Host ""
    Write-Host "WARNING: A clean reinstall deletes the existing 'polis' VM and removes previous workspace data." -ForegroundColor Red
    if ($env:POLIS_INSTALL_ASSUME_Y -eq "1" -or $env:CI -eq "true" -or $env:GITHUB_ACTIONS -eq "true" -or -not [Environment]::UserInteractive) {
        Write-Host "[INFO] Non-interactive mode detected — proceeding automatically."
        return
    }
    $reply = (Read-Host "Continue with installation? (y/n)").Trim().ToLowerInvariant()
    if ($reply -notin @("y", "yes")) {
        Write-Warn "Installation cancelled by user."
        exit 1
    }
}

function Write-Logo {
    $esc = [char]27
    $c = @(
        "$esc[38;2;107;33;168m",
        "$esc[38;2;93;37;163m",
        "$esc[38;2;64;47;153m",
        "$esc[38;2;46;53;147m",
        "$esc[38;2;37;56;144m",
        "$esc[38;2;26;107;160m",
        "$esc[38;2;26;151;179m",
        "$esc[38;2;20;184;166m"
    )
    $x = "$esc[0m"
    $rows = @(
        @(" ▄████▄ ", "█████▄ ", "█████▄ ", " ▄████▄ ", "  ██      ", " ▄████▄ ", "█████▄ ", " ▄████▄"),
        @("██    ██", "██   ██", "██   ██", "██    ██", "  ██      ", "██    ██", "██   ██", "██     "),
        @("██    ██", "██   ██", "██   ██", "██    ██", "  ██      ", "██    ██", "██   ██", "██     "),
        @("██    ██", "██   ██", "█████▀ ", "████████", "  ██      ", "████████", "█████▀ ", " ▀████▄"),
        @("██    ██", "██   ██", "██  ██ ", "██    ██", "  ██      ", "██    ██", "██   ██", "      ██"),
        @("██    ██", "██   ██", "██   ██", "██    ██", "  ██      ", "██    ██", "██   ██", "      ██"),
        @(" ▀████▀ ", "█████▀ ", "██   ██", "██    ██", "  ████████", "██    ██", "█████▀ ", " ▀████▀")
    )
    Write-Host ""
    foreach ($row in $rows) {
        $line = ""
        for ($i = 0; $i -lt $row.Count; $i++) {
            $line += "$($c[$i])$($row[$i])$x "
        }
        Write-Host $line
    }
    Write-Host ""
}

# -- Multipass -----------------------------------------------------------------

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
    $msi = Join-Path $env:TEMP "multipass-${MultipassVersion}+win-win64.msi"
    $url = "https://github.com/canonical/multipass/releases/download/v${MultipassVersion}/multipass-${MultipassVersion}+win-win64.msi"
    Write-Info "Downloading Multipass ${MultipassVersion}..."
    Invoke-WebRequest -Uri $url -OutFile $msi -UseBasicParsing

    # SHA256 hash for Multipass MSI — update when bumping $MultipassVersion
    $MultipassSha256 = if ($env:MULTIPASS_SHA256_WIN) { $env:MULTIPASS_SHA256_WIN } else { "5b697c4312f2267041adf001ba750ff8b8fcf4fd68675493661cb30af742f283" }

    Write-Info "Verifying Multipass MSI SHA256..."
    $msiHash = (Get-FileHash $msi -Algorithm SHA256).Hash.ToLower()
    if ($msiHash -ne $MultipassSha256.ToLower()) {
        Write-Err "Multipass MSI SHA256 mismatch!"
        Write-Host "  Expected: $MultipassSha256"
        Write-Host "  Actual:   $msiHash"
        Remove-Item $msi -Force -ErrorAction SilentlyContinue
        throw "Multipass MSI SHA256 mismatch."
    }
    Write-Ok "Multipass MSI SHA256 verified"

    Write-Info "Installing Multipass (this may take a minute)..."
    $proc = Start-Process msiexec -ArgumentList "/i `"$msi`" /quiet /norestart" -Wait -PassThru
    Remove-Item $msi -Force -ErrorAction SilentlyContinue
    if ($proc.ExitCode -ne 0) {
        throw "Multipass installer exited with code $($proc.ExitCode)."
    }
    $env:PATH = [System.Environment]::GetEnvironmentVariable("PATH", "Machine") + ";" +
                [System.Environment]::GetEnvironmentVariable("PATH", "User")
}

function Assert-Multipass {
    if (-not (Get-Command multipass -ErrorAction SilentlyContinue)) {
        Write-Info "Multipass not found - installing..."
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
            throw "Missing virtualization backend."
        }
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
                throw "Multipass version too old."
            }
            Write-Ok "Multipass $verStr OK"
        } catch {
            Write-Warn "Could not parse Multipass version '$verStr' - proceeding anyway."
        }
    }
}

function Assert-Architecture {
    if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") {
        Write-Err "Unsupported architecture: $env:PROCESSOR_ARCHITECTURE"
        Write-Host "  Polis currently requires x86_64 (AMD64)."
        throw "Unsupported architecture."
    }
}

# -- CLI -----------------------------------------------------------------------

function Resolve-Version {
    if ($env:POLIS_VERSION) {
        $script:Version = $env:POLIS_VERSION -replace '^v', ''
        return
    }
    Write-Info "Detecting latest Polis release..."
    # Use releases API (returns pre-releases too, unlike /releases/latest)
    $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/${RepoOwner}/${RepoName}/releases?per_page=1" -UseBasicParsing
    if (-not $releases -or $releases.Count -eq 0) {
        Write-Err "Could not detect latest version from GitHub."
        Write-Host '  Set POLIS_VERSION manually: $env:POLIS_VERSION="0.4.0"; .\install.ps1'
        throw "Version detection failed."
    }
    $tag = $releases[0].tag_name
    $script:Version = $tag -replace '^v', ''
    Write-Ok "Detected version: $script:Version"
}

function Install-Cli {
    $binDir = Join-Path $InstallDir "bin"
    New-Item -ItemType Directory -Force -Path $binDir | Out-Null

    $base = "https://github.com/${RepoOwner}/${RepoName}/releases/download/v${Version}"
    $exe  = Join-Path $binDir "polis.exe"
    $sha  = Join-Path $env:TEMP "polis.sha256"

    Write-Info "Downloading CLI ${Version}..."
    Invoke-WebRequest -Uri "$base/polis-windows-amd64.exe"    -OutFile $exe -UseBasicParsing
    Invoke-WebRequest -Uri "$base/polis-windows-amd64.sha256" -OutFile $sha -UseBasicParsing

    Write-Info "Verifying CLI SHA256..."
    $expected = (Get-Content $sha -Raw).Trim().Split()[0]
    $actual   = (Get-FileHash $exe -Algorithm SHA256).Hash.ToLower()
    Remove-Item $sha -Force -ErrorAction SilentlyContinue
    if ($actual -ne $expected.ToLower()) {
        Write-Err "SHA256 mismatch!"
        Write-Host "  Expected: $expected"
        Write-Host "  Actual:   $actual"
        Remove-Item $exe -Force -ErrorAction SilentlyContinue
        throw "CLI SHA256 mismatch."
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

# -- Main ----------------------------------------------------------------------

function Invoke-PolisInstall {
    $ErrorActionPreference = "Stop"
    $ProgressPreference = "SilentlyContinue"

    Write-Logo

    Write-Host ""
    Write-Host "+===============================================================+"
    Write-Host "|                    Polis Installer                            |"
    Write-Host "+===============================================================+"
    Write-Host ""

    Confirm-InstallerProceed
    Assert-Architecture
    Assert-Multipass
    Show-WindowsNetworkingNote
    Resolve-Version
    Write-Info "Installing Polis ${Version}"
    Install-Cli
    Add-ToUserPath

    # Delete existing VM for a clean install
    $vmExists = $false
    try {
        $ErrorActionPreference = "Continue"
        $null = & multipass info polis 2>&1
        if ($LASTEXITCODE -eq 0) { $vmExists = $true }
        $ErrorActionPreference = "Stop"
    } catch {
        $ErrorActionPreference = "Stop"
    }

    $polis = Join-Path $InstallDir "bin\polis.exe"

    if ($vmExists) {
        Write-Warn "Existing polis VM found — deleting..."
        $ErrorActionPreference = "Continue"
        & multipass stop polis --force 2>$null
        & multipass delete polis --purge
        $ErrorActionPreference = "Stop"
        # Verify deletion succeeded
        $null = & multipass info polis 2>&1
        if ($LASTEXITCODE -eq 0) {
            Write-Err "Failed to delete existing VM. Try manually:"
            Write-Host "  multipass stop polis; multipass delete polis --purge"
            throw "VM deletion failed."
        }
        Write-Ok "VM deleted"
    }

    Remove-Item (Join-Path $InstallDir "state.json") -Force -ErrorAction SilentlyContinue

    # Start (creates VM, generates certs inside VM)
    Write-Info "Starting Polis..."
    $ErrorActionPreference = "Continue"
    & $polis start
    $startExitCode = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($startExitCode -ne 0) {
        Write-Err "polis start failed."
        throw "polis start failed."
    }

    Write-Host ""
    Write-Ok "Polis installed successfully!"
    Write-Host ""
}

try {
    Invoke-PolisInstall
} catch {
    Write-Err $_.Exception.Message
}
