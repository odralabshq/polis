# build-vm-hyperv.ps1 — Build Polis Hyper-V VM image on Windows
# Usage: .\build-vm-hyperv.ps1 [-Arch amd64] [-Headless true]
[CmdletBinding()]
param(
    [string]$Arch = "amd64",
    [string]$Headless = "true"
)

# Ensure oscdimg (required by Packer's cd_content for seed ISO creation) is on PATH.
# It ships with the Windows ADK — Deployment Tools feature.
$oscdimg = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10" -Recurse -Filter "oscdimg.exe" -ErrorAction SilentlyContinue |
Where-Object { $_.FullName -match "amd64" } | Select-Object -First 1
if ($oscdimg) {
    $env:PATH = "$($oscdimg.DirectoryName);$env:PATH"
    Write-Host "==> oscdimg found: $($oscdimg.FullName)"
}
else {
    Write-Warning "oscdimg not found. Install Windows ADK (Deployment Tools): winget install Microsoft.WindowsADK"
}
$ErrorActionPreference = "Stop"

$ROOT = (Get-Location).Path

# Resolve absolute paths for tarballs
$ImagesTar = Join-Path $ROOT ".build\polis-images.tar"
$ConfigTar = Join-Path $ROOT ".build\polis-config.tar.gz"
$AgentsTar = Join-Path $ROOT ".build\polis-agents.tar.gz"

Push-Location (Join-Path $ROOT "packer")
try {
    if (Test-Path "output-hyperv") { Remove-Item -Recurse -Force "output-hyperv" }

    Write-Host "==> Initialising Packer plugins..."
    packer init .
    if ($LASTEXITCODE -ne 0) { throw "packer init failed" }

    Write-Host "==> Building Hyper-V image (arch=$Arch, headless=$Headless)..."
    packer build `
        -only "hyperv-iso.polis" `
        -var "images_tar=$ImagesTar" `
        -var "config_tar=$ConfigTar" `
        -var "agents_tar=$AgentsTar" `
        -var "arch=$Arch" `
        -var "headless=$Headless" `
        polis-vm.pkr.hcl
    if ($LASTEXITCODE -ne 0) { throw "packer build failed" }
}
finally {
    Pop-Location
}
