# build-agents-windows.ps1 — Bundle agent artifacts for VM image (Windows version)
# Usage: .\build-agents-windows.ps1
$ErrorActionPreference = "Stop"

if (Test-Path ".build\agents") { Remove-Item -Recurse -Force ".build\agents" }
New-Item -ItemType Directory -Path ".build\agents" -Force | Out-Null

Write-Host "==> Copying agent directories..."
Get-ChildItem "agents" | ForEach-Object {
    if ($_.PSIsContainer -and $_.Name -ne "_template" -and (Test-Path (Join-Path $_.FullName "agent.yaml"))) {
        Copy-Item $_.FullName ".build\agents" -Recurse
    }
}

Write-Host "==> Generating agent artifacts..."
Get-ChildItem ".build\agents" | ForEach-Object {
    if ($_.PSIsContainer) {
        bash ./scripts/generate-agent.sh $_.Name .build/agents
    }
}

tar -czf ".build\polis-agents.tar.gz" -C ".build" agents
Write-Host "==> Agents bundle: .build\polis-agents.tar.gz"
