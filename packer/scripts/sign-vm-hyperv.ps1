# sign-vm-hyperv.ps1 — Sign the Hyper-V .vhdx image with a dev keypair
# Usage: .\sign-vm-hyperv.ps1 [-Arch amd64]
[CmdletBinding()]
param(
    [string]$Arch = "amd64"
)
$ErrorActionPreference = "Stop"

$SigningKey = ".secrets\polis-release.key"
$PubKey = ".secrets\polis-release.pub"

if (!(Test-Path $SigningKey)) {
    Write-Host "No signing key found — generating dev keypair at $SigningKey..."
    New-Item -ItemType Directory -Path ".secrets" -Force | Out-Null
    & zipsign gen-key $SigningKey $PubKey
    Write-Host "✓ Dev keypair generated (gitignored)"
}

$Image = Get-ChildItem -Path "packer\output-hyperv" -Filter "*-$Arch.vhdx" `
| Sort-Object LastWriteTime -Descending | Select-Object -First 1

if (!$Image) {
    Write-Error "ERROR: No .vhdx found in packer\output-hyperv"
    exit 1
}

$Checksum = (Get-FileHash $Image.FullName -Algorithm SHA256).Hash.ToLower()
$Tmp = New-Item -ItemType Directory -Path (Join-Path $env:TEMP ([System.IO.Path]::GetRandomFileName()))

try {
    "$($Image.Name)  $Checksum" | Set-Content -Path (Join-Path $Tmp "checksum.txt")
    tar -czf (Join-Path $Tmp "sidecar.tar.gz") -C $Tmp checksum.txt
    & zipsign sign tar --context "" (Join-Path $Tmp "sidecar.tar.gz") $SigningKey -o "$($Image.FullName).sha256" -f
    Write-Host "✓ $($Image.Name).sha256 (signed)"
    Write-Host "  Public key: $([Convert]::ToBase64String([System.IO.File]::ReadAllBytes($PubKey)))"
}
finally {
    Remove-Item -Recurse -Force $Tmp
}
