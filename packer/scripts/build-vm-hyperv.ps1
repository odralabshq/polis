# build-vm-hyperv.ps1 - Build Polis Hyper-V VM image on Windows
# Usage: .\build-vm-hyperv.ps1 [-Arch amd64] [-Headless true]
[CmdletBinding()]
param(
    [string]$Arch = "amd64",
    [string]$Headless = "true"
)

$ErrorActionPreference = "Stop"

# oscdimg needed by Packer for CIDATA seed ISO - add to PATH if present
$oscdimg = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10" -Recurse -Filter "oscdimg.exe" -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match "amd64" } | Select-Object -First 1
if ($oscdimg) { $env:PATH = "$($oscdimg.DirectoryName);$env:PATH" }

$ROOT = (Get-Location).Path

# Firewall rule for Packer HTTP server (serves autoinstall user-data to the VM)
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$ruleExists = Get-NetFirewallRule -DisplayName "Packer_http_server" -ErrorAction SilentlyContinue
if (-not $ruleExists) {
    if ($isAdmin) {
        New-NetFirewallRule -DisplayName "Packer_http_server" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 8000-9000 | Out-Null
        Write-Host "==> Firewall rule created"
    } else {
        Write-Warning "Firewall rule missing - run once as admin: New-NetFirewallRule -DisplayName 'Packer_http_server' -Direction Inbound -Action Allow -Protocol TCP -LocalPort 8000-9000"
    }
}

$ImagesTar = Join-Path $ROOT ".build\polis-images.tar"
$ConfigTar = Join-Path $ROOT ".build\polis-config.tar.gz"
$AgentsTar = Join-Path $ROOT ".build\polis-agents.tar.gz"

Push-Location (Join-Path $ROOT "packer")
try {
    if (Test-Path "output-hyperv") { Remove-Item -Recurse -Force "output-hyperv" }

    # Remove any leftover VM from a previous failed run
    $vmName = "polis-dev-$Arch"
    $oldVm = Get-VM -Name $vmName -ErrorAction SilentlyContinue
    if ($oldVm) {
        Write-Host "==> Removing leftover VM: $vmName"
        Stop-VM -Name $vmName -TurnOff -Force -ErrorAction SilentlyContinue
        Start-Sleep 3
        Remove-VM -Name $vmName -Force -ErrorAction SilentlyContinue
    }

    Write-Host "==> Initialising Packer plugins..."
    & packer init .
    if ($LASTEXITCODE -ne 0) { throw "packer init failed" }

    Write-Host "==> Building Hyper-V image (arch=$Arch headless=$Headless)..."
    $packerArgs = @(
        "build",
        "-only", "hyperv-iso.polis",
        "-var", "images_tar=$ImagesTar",
        "-var", "config_tar=$ConfigTar",
        "-var", "agents_tar=$AgentsTar",
        "-var", "arch=$Arch",
        "-var", "headless=$Headless",
        "polis-vm.pkr.hcl"
    )
    & packer @packerArgs
    if ($LASTEXITCODE -ne 0) { throw "packer build failed" }

    # Compact the VHDX after Packer exports it (Packer's built-in Optimize-VHD
    # fails because it runs before the file is in the output directory).
    $vhdx = Get-ChildItem "output-hyperv" -Filter "*.vhdx" -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($vhdx) {
        $before = $vhdx.Length
        Write-Host "==> Compacting $($vhdx.Name)..."
        Optimize-VHD -Path $vhdx.FullName -Mode Full
        $after = (Get-Item $vhdx.FullName).Length
        Write-Host "==> Compacted: $([math]::Round($before/1MB)) MB -> $([math]::Round($after/1MB)) MB"
    }
}
finally {
    Pop-Location
}
