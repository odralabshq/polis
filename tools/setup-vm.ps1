# setup-vm.ps1 - Automated Polis VM Setup and SSH Authorization
# This script creates the development VM and authorizes your local SSH key for IDE access.

param(
    [string]$Name = $(if ($env:POLIS_NAME) { $env:POLIS_NAME } else { "polis-dev" }),
    [string]$Cpus = $(if ($env:POLIS_CPUS) { $env:POLIS_CPUS } else { "4" }),
    [string]$Memory = $(if ($env:POLIS_MEMORY) { $env:POLIS_MEMORY } else { "16G" }),
    [string]$Disk = $(if ($env:POLIS_DISK) { $env:POLIS_DISK } else { "50G" })
)

$ErrorActionPreference = "Stop"

function Write-Step { Write-Host "`n[STEP] $args" -ForegroundColor Cyan }
function Write-Info { Write-Host "[INFO] $args" -ForegroundColor Blue }
function Write-Success { Write-Host "[OK] $args" -ForegroundColor Green }

# 1. Check Multipass
if (-not (Get-Command multipass -ErrorAction SilentlyContinue)) {
    Write-Error "Multipass is not installed. Please install it first (winget install Canonical.Multipass)."
}

# 2. Check/Generate SSH Key
$SshDir = Join-Path $HOME ".ssh"
$PubKeyFile = Join-Path $SshDir "id_ed25519.pub"
if (-not (Test-Path $PubKeyFile)) {
    $PubKeyFile = Join-Path $SshDir "id_rsa.pub"
}

if (-not (Test-Path $PubKeyFile)) {
    Write-Step "Generating SSH Key..."
    if (-not (Test-Path $SshDir)) { New-Item -Path $SshDir -ItemType Directory }
    ssh-keygen -t ed25519 -f (Join-Path $SshDir "id_ed25519") -N ""
    $PubKeyFile = Join-Path $SshDir "id_ed25519.pub"
    Write-Success "Generated new SSH key: $PubKeyFile"
}
else {
    Write-Info "Using existing SSH key: $PubKeyFile"
}

$PubKey = Get-Content $PubKeyFile -Raw

# 3. Create VM
Write-Step "Creating VM '$Name'..."
$vms = multipass list
if ($vms -match "^$Name\s") {
    Write-Info "VM '$Name' already exists. Skipping creation."
}
else {
    multipass launch `
        --name $Name `
        --cpus $Cpus `
        --memory $Memory `
        --disk $Disk `
        --cloud-init polis-dev.yaml `
        24.04
    Write-Success "VM launched."
}

# 4. Wait for Cloud-Init
Write-Step "Waiting for VM to be ready..."
multipass exec $Name -- cloud-init status --wait
Write-Success "VM is ready."

# 5. Authorize Key
Write-Step "Authorizing your host's SSH key..."
multipass exec $Name -- bash -c "mkdir -p ~/.ssh && echo '$PubKey' >> ~/.ssh/authorized_keys"
Write-Success "Key authorized."

# 6. Get IP and Show Config
$Ip = (multipass info $Name --format json | ConvertFrom-Json).info.$Name.ipv4[0]

Write-Host "`n====================================================" -ForegroundColor Yellow
Write-Host "Setup Complete!" -ForegroundColor Green
Write-Host "====================================================`n" -ForegroundColor Yellow

Write-Host "SSH CONFIGURATION FOR VS CODE:" -ForegroundColor Cyan
Write-Host "-------------------------------"
Write-Host "Host $Name"
Write-Host "    HostName $Ip"
Write-Host "    User ubuntu"
Write-Host "    IdentityFile $($PubKeyFile.Replace('.pub', ''))"
Write-Host "-------------------------------`n"

Write-Host "Next Steps:"
Write-Host "1. Open VS Code."
Write-Host "2. Connect to Host '$Name' (Remote-SSH)."
Write-Host "3. Open folder '~/polis'."
Write-Host "4. Inside the terminal, run: ./cli/polis.sh init --local"
