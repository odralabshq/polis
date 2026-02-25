# bundle-config-windows.ps1 — Bundle Polis config for VM image (Windows version)
# Usage: .\bundle-config-windows.ps1
$ErrorActionPreference = "Stop"

$POLIS_IMAGE_VERSION = if ($env:POLIS_IMAGE_VERSION) { $env:POLIS_IMAGE_VERSION } else { "latest" }
$BUNDLE_DIR = New-Item -ItemType Directory -Path (Join-Path $env:TEMP ([System.IO.Path]::GetRandomFileName()))

try {
    Write-Host "==> Bundling Polis configuration (version=$POLIS_IMAGE_VERSION)..."

    # Strip @sha256:... digest suffixes
    Get-Content docker-compose.yml |
    ForEach-Object { $_ -replace '@sha256:[a-f0-9]{64}', '' } |
    Set-Content (Join-Path $BUNDLE_DIR "docker-compose.yml")

    # Write .env with pinned versions
    @"
POLIS_RESOLVER_VERSION=$POLIS_IMAGE_VERSION
POLIS_CERTGEN_VERSION=$POLIS_IMAGE_VERSION
POLIS_GATE_VERSION=$POLIS_IMAGE_VERSION
POLIS_SENTINEL_VERSION=$POLIS_IMAGE_VERSION
POLIS_SCANNER_VERSION=$POLIS_IMAGE_VERSION
POLIS_WORKSPACE_VERSION=$POLIS_IMAGE_VERSION
POLIS_HOST_INIT_VERSION=$POLIS_IMAGE_VERSION
POLIS_STATE_VERSION=$POLIS_IMAGE_VERSION
POLIS_TOOLBOX_VERSION=$POLIS_IMAGE_VERSION
"@ | Set-Content (Join-Path $BUNDLE_DIR ".env")

    # Copy service configs and scripts
    New-Item -ItemType Directory -Path (Join-Path $BUNDLE_DIR "services") -Force | Out-Null
    foreach ($svc in @("resolver", "certgen", "gate", "sentinel", "scanner", "state", "toolbox", "workspace")) {
        if (Test-Path "services\$svc\config") {
            New-Item -ItemType Directory -Path (Join-Path $BUNDLE_DIR "services\$svc") -Force | Out-Null
            Copy-Item -Path "services\$svc\config" -Destination (Join-Path $BUNDLE_DIR "services\$svc") -Recurse
        }
        if (Test-Path "services\$svc\scripts") {
            New-Item -ItemType Directory -Path (Join-Path $BUNDLE_DIR "services\$svc") -Force | Out-Null
            Copy-Item -Path "services\$svc\scripts" -Destination (Join-Path $BUNDLE_DIR "services\$svc") -Recurse
        }
    }

    # Copy setup scripts
    New-Item -ItemType Directory -Path (Join-Path $BUNDLE_DIR "scripts") -Force | Out-Null
    Copy-Item "packer\scripts\setup-certs.sh" (Join-Path $BUNDLE_DIR "scripts")
    Copy-Item "scripts\generate-agent.sh"     (Join-Path $BUNDLE_DIR "scripts")

    # Copy config
    New-Item -ItemType Directory -Path (Join-Path $BUNDLE_DIR "config") -Force | Out-Null
    Copy-Item "config\polis.yaml" (Join-Path $BUNDLE_DIR "config")

    # Create placeholder directories
    New-Item -ItemType Directory -Path (Join-Path $BUNDLE_DIR "certs\ca")      -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $BUNDLE_DIR "certs\valkey")  -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $BUNDLE_DIR "certs\toolbox") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $BUNDLE_DIR "secrets")       -Force | Out-Null

    # Create tarball
    New-Item -ItemType Directory -Path ".build" -Force | Out-Null
    tar -czf ".build\polis-config.tar.gz" -C $BUNDLE_DIR .
    Write-Host "==> Bundle created: .build\polis-config.tar.gz"
}
finally {
    Remove-Item -Recurse -Force $BUNDLE_DIR
}
