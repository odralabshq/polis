# save-docker-images-windows.ps1 — Save Docker images as compressed tarball (Windows)
# Equivalent of the save-docker-images just recipe, using PowerShell + zstd from Anaconda/PATH.
[CmdletBinding()]
param(
    [string]$RepoDir = (Split-Path $PSScriptRoot -Parent)
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Set-Location $RepoDir

New-Item -ItemType Directory -Force -Path ".build" | Out-Null

# Get CLI version from Cargo.toml metadata
$cargoJson = cargo metadata --no-deps --format-version 1 --manifest-path cli/Cargo.toml 2>&1
if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed" }
$version = "v$(($cargoJson | ConvertFrom-Json).packages[0].version)"
Write-Host "[INFO] CLI version: $version" -ForegroundColor Cyan

# Get all compose images
$images = (docker compose config --images) | Sort-Object -Unique
Write-Host "[INFO] Found $($images.Count) images" -ForegroundColor Cyan

# Tag images with version
foreach ($img in $images) {
    $base = ($img -split ':')[0]
    if ($base -ne $img) {
        $inspectResult = docker image inspect "${base}:latest" 2>$null
        if ($LASTEXITCODE -eq 0) {
            docker tag "${base}:latest" "${base}:${version}"
        }
    }
}

# Find zstd — check PATH first, then common locations
$zstd = Get-Command zstd -ErrorAction SilentlyContinue
if (-not $zstd) {
    # Check Anaconda
    $anacondaZstd = "$env:USERPROFILE\anaconda3\Library\bin\zstd.exe"
    if (Test-Path $anacondaZstd) { $zstd = Get-Item $anacondaZstd }
}
if (-not $zstd) {
    throw "zstd not found. Install via: winget install Meta.Zstandard  (or use Anaconda)"
}
$zstdPath = if ($zstd -is [System.Management.Automation.ApplicationInfo]) { $zstd.Source } else { $zstd.FullName }
Write-Host "[INFO] Using zstd: $zstdPath" -ForegroundColor Cyan

# Save and compress
$tarZst = ".build\polis-images.tar.zst"
Write-Host "[INFO] Saving $($images.Count) images..." -ForegroundColor Cyan

# docker save piped to zstd
$imageList = $images -join " "
$cmd = "docker save $imageList | `"$zstdPath`" -T0 -3 -o `"$tarZst`" --force"
cmd /c $cmd
if ($LASTEXITCODE -ne 0) { throw "docker save | zstd failed" }

$size = [math]::Round((Get-Item $tarZst).Length / 1MB, 1)
Write-Host "[OK] Saved $tarZst (${size} MB)" -ForegroundColor Green
