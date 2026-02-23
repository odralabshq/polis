# =============================================================================
# Polis Installer for Windows
# =============================================================================
# One-line install: irm https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.ps1 | iex
# =============================================================================
Set-StrictMode -Version Latest

# Ensure TLS 1.2 for GitHub downloads (PS 5.1 defaults to TLS 1.0)
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Version          = if ($env:POLIS_VERSION)  { $env:POLIS_VERSION }  else { "0.3.0-preview-8" }
$InstallDir       = if ($env:POLIS_HOME)     { $env:POLIS_HOME }     else { Join-Path $env:USERPROFILE ".polis" }
$CdnBaseUrl       = if ($env:POLIS_CDN_URL)  { $env:POLIS_CDN_URL }  else { "https://d1qggvwquwdnma.cloudfront.net" }
$ImageDir         = Join-Path $env:ProgramData "Polis\images"
$RepoOwner        = "OdraLabsHQ"
$RepoName         = "polis"
$MultipassMin     = [version]"1.16.0"
$MultipassVersion = "1.16.1"

function Write-Info { param($msg) Write-Host "[INFO]  $msg" -ForegroundColor Cyan }
function Write-Ok   { param($msg) Write-Host "[OK]    $msg" -ForegroundColor Green }
function Write-Warn { param($msg) Write-Host "[WARN]  $msg" -ForegroundColor Yellow }
function Write-Err  { param($msg) Write-Host "[ERROR] $msg" -ForegroundColor Red }

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
    $msi = Join-Path $env:TEMP "multipass-${MultipassVersion}+win-Windows.msi"
    $url = "https://github.com/canonical/multipass/releases/download/v${MultipassVersion}/multipass-${MultipassVersion}+win-Windows.msi"
    Write-Info "Downloading Multipass ${MultipassVersion}..."
    Invoke-WebRequest -Uri $url -OutFile $msi -UseBasicParsing
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

# -- CLI -----------------------------------------------------------------------

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

# -- Image ---------------------------------------------------------------------

function Get-Image {
    $ghBase    = "https://github.com/${RepoOwner}/${RepoName}/releases/download/${Version}"
    $versions  = Invoke-RestMethod -Uri "$ghBase/versions.json" -UseBasicParsing
    $imageName = $versions.vm_image.asset
    $dest      = Join-Path $ImageDir $imageName
    $sidecar   = "$dest.sha256"

    New-Item -ItemType Directory -Force -Path $ImageDir | Out-Null

    # Try CDN first, fall back to GitHub (S3 keys use v-prefixed version)
    $cdnUrl     = "${CdnBaseUrl}/v${Version}/${imageName}"
    $ghUrl      = "${ghBase}/${imageName}"
    $downloaded = $false

    Write-Info "Downloading VM image from CDN (this may take a few minutes)..."
    try {
        Invoke-WebRequest -Uri $cdnUrl -OutFile $dest -UseBasicParsing -ErrorAction Stop
        $downloaded = $true
    } catch {
        Write-Warn "CDN unavailable, falling back to GitHub..."
    }

    if (-not $downloaded) {
        Write-Info "Downloading VM image from GitHub (this may take a few minutes)..."
        Invoke-WebRequest -Uri $ghUrl -OutFile $dest -UseBasicParsing
    }

    # Download signed sidecar for CLI integrity verification
    Invoke-WebRequest -Uri "$ghBase/$imageName.sha256" -OutFile $sidecar -UseBasicParsing

    # Checksum always fetched from GitHub (separate origin from binary)
    Write-Info "Verifying image SHA256..."
    $checksumFile = Join-Path $env:TEMP "polis-checksums.sha256"
    Invoke-WebRequest -Uri "$ghBase/checksums.sha256" -OutFile $checksumFile -UseBasicParsing
    $expected = (Get-Content $checksumFile | Where-Object { $_ -like "*$imageName*" } | ForEach-Object { ($_ -split '\s+')[0] }) | Select-Object -First 1
    Remove-Item $checksumFile -Force -ErrorAction SilentlyContinue
    if (-not $expected) {
        Write-Warn "Could not find checksum for $imageName - skipping verification"
    } else {
        $actual = (Get-FileHash $dest -Algorithm SHA256).Hash.ToLower()
        if ($actual -ne $expected.ToLower()) {
            Write-Err "Image SHA256 mismatch!"
            Write-Host "  Expected: $expected"
            Write-Host "  Actual:   $actual"
            Remove-Item $dest -Force -ErrorAction SilentlyContinue
            Remove-Item $sidecar -Force -ErrorAction SilentlyContinue
            throw "Image SHA256 mismatch."
        }
        Write-Ok "Image SHA256 verified: $expected"
    }
    return $dest
}

# -- Init ----------------------------------------------------------------------

function Invoke-PolisInit {
    param([string]$ImagePath)
    $polis = Join-Path $InstallDir "bin\polis.exe"

    # Check if a polis VM already exists (suppress all errors — expected to fail if no VM)
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
            Write-Info "Keeping existing VM. Skipping removal."
        }
    }
    Remove-Item (Join-Path $InstallDir "state.json") -Force -ErrorAction SilentlyContinue

    Write-Info "Running: polis start --image $ImagePath"
    $ErrorActionPreference = "Continue"
    & $polis start --image $ImagePath
    $startExitCode = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($startExitCode -ne 0) {
        Write-Err "polis start failed. Run manually:"
        Write-Host "  polis start --image $ImagePath"
        throw "polis start failed."
    }
}

# -- Main ----------------------------------------------------------------------

function Invoke-PolisInstall {
    $ErrorActionPreference = "Stop"
    # Suppress progress bars — makes Invoke-WebRequest 10-50x faster on PS 5.x
    $ProgressPreference = "SilentlyContinue"

    Write-Host ""
    Write-Host "+===============================================================+"
    Write-Host "|                    Polis Installer                            |"
    Write-Host "+===============================================================+"
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
}

try {
    Invoke-PolisInstall
} catch {
    Write-Err $_.Exception.Message
}
