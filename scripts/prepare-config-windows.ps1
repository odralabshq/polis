# prepare-config-windows.ps1 — Windows equivalent of the prepare-config just recipe
# No sudo required. Uses built-in Windows tar (available since Windows 10 1803).
[CmdletBinding()]
param(
    [string]$RepoDir = (Split-Path $PSScriptRoot -Parent)
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Step { param($msg) Write-Host "→ $msg" -ForegroundColor Cyan }
function Write-Ok   { param($msg) Write-Host "✓ $msg" -ForegroundColor Green }
function Write-Warn { param($msg) Write-Host "[WARN] $msg" -ForegroundColor Yellow }

Set-Location $RepoDir

# ── Agent artifact generation ─────────────────────────────────────────────────

Write-Step "Generating agent artifacts..."
Get-ChildItem "agents" -Directory | Where-Object { $_.Name -ne "_template" } | ForEach-Object {
    $agentDir = $_.FullName
    $name = $_.Name
    if (Test-Path "$agentDir\agent.yaml") {
        Write-Step "Generating artifacts for agent: $name"
        # generate-agent.sh runs via Git Bash / WSL bash
        $bash = Get-Command bash -ErrorAction SilentlyContinue
        if ($bash) {
            & bash scripts/generate-agent.sh $name agents
            if ($LASTEXITCODE -ne 0) { throw "generate-agent.sh failed for $name" }
            Write-Ok "Generated artifacts for '$name'"
        } else {
            Write-Warn "bash not found — skipping agent artifact generation for $name"
        }
    }
}

# ── Config tarball ────────────────────────────────────────────────────────────

Write-Step "Building config tarball..."
New-Item -ItemType Directory -Force -Path ".build\assets" | Out-Null

$tarDest = ".build\assets\polis-setup.config.tar"

# Collect paths to include (only those that exist)
$includes = @(
    "docker-compose.yml",
    "scripts",
    "agents"
)

Get-ChildItem "services" -Directory | ForEach-Object {
    if (Test-Path "$($_.FullName)\config")  { $includes += "services/$($_.Name)/config" }
    if (Test-Path "$($_.FullName)\scripts") { $includes += "services/$($_.Name)/scripts" }
}

if (Test-Path "certs")   { $includes += "certs" }
if (Test-Path "secrets") { $includes += "secrets" }

# tar on Windows doesn't support --force-local but works fine for creation
# Use -c (create), -f (file) — no sudo needed
$tarArgs = @("-cf", $tarDest) + $includes
& tar @tarArgs
if ($LASTEXITCODE -ne 0) { throw "tar failed" }
Write-Ok "Built $tarDest"

# ── cloud-init.yaml ───────────────────────────────────────────────────────────

Copy-Item "cloud-init.yaml" ".build\assets\cloud-init.yaml" -Force
Write-Ok "Copied cloud-init.yaml"

# ── image-digests.json stub ───────────────────────────────────────────────────

$digestPath = ".build\assets\image-digests.json"
if (-not (Test-Path $digestPath)) {
    Set-Content $digestPath "{}"
    Write-Ok "Created stub image-digests.json"
}

Write-Host ""
Write-Ok "prepare-config complete"
