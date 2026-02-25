# export-images-windows.ps1 — Export Docker images to a tar for VM build (Windows version)
# Usage: .\export-images-windows.ps1
$ErrorActionPreference = "Stop"

$POLIS_IMAGE_VERSION = if ($env:POLIS_IMAGE_VERSION) { $env:POLIS_IMAGE_VERSION } else { "latest" }

# Set per-service version env vars so docker compose config resolves image refs
$services = @("RESOLVER", "CERTGEN", "GATE", "SENTINEL", "SCANNER", "WORKSPACE", "HOST_INIT", "STATE", "TOOLBOX")
foreach ($svc in $services) {
    [System.Environment]::SetEnvironmentVariable("POLIS_${svc}_VERSION", $POLIS_IMAGE_VERSION, "Process")
}

Write-Host "==> Resolving image list from docker-compose.yml..."
$composeConfig = docker compose -f docker-compose.yml config 2>&1
if ($LASTEXITCODE -ne 0) { throw "docker compose config failed" }

# Extract image references
$Images = ($composeConfig | Select-String -Pattern '^\s+image:\s+(.+)$').Matches |
ForEach-Object { $_.Groups[1].Value.Trim() } |
Sort-Object -Unique |
Where-Object { $_ -notmatch 'go-httpbin' }

if (!$Images) {
    Write-Error "ERROR: No images found in docker-compose.yml"
    exit 1
}

New-Item -ItemType Directory -Path ".build" -Force | Out-Null

Write-Host "==> Pulling external images..."
$ExportImages = @()

foreach ($img in $Images) {
    if ($img -notmatch '^ghcr\.io/odralabshq/polis-') {
        Write-Host "  Pulling $img..."
        docker pull $img 2>&1 | Out-Null
        # Strip @sha256:... suffix (docker load doesn't preserve digests)
        $simpleTag = $img -replace '@sha256:[a-f0-9]{64}', ''
        if ($simpleTag -ne $img) {
            Write-Host "  Tagging $img as $simpleTag"
            docker tag $img $simpleTag
            $ExportImages += $simpleTag
        }
        else {
            $ExportImages += $img
        }
    }
    else {
        $ExportImages += $img
    }
}

Write-Host "==> Exporting $($ExportImages.Count) images to .build/polis-images.tar..."
$exportArgs = @("save", "-o", ".build\polis-images.tar") + $ExportImages
& docker @exportArgs
if ($LASTEXITCODE -ne 0) { throw "docker save failed" }

$size = (Get-Item ".build\polis-images.tar").Length / 1MB
Write-Host ("==> Done. .build\polis-images.tar ({0:N0} MB)" -f $size)
