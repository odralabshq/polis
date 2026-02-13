# polis-dev.ps1 - Automated Polis Development Environment Setup (Windows)
# Creates a Multipass VM with Docker + Sysbox and mounts your local Polis directory

param(
    [Parameter(Position=0)]
    [string]$Command,
    
    [string]$Name = $env:POLIS_DEV_VM_NAME,
    [int]$Cpus = $env:POLIS_DEV_CPUS,
    [string]$Memory = $env:POLIS_DEV_MEMORY,
    [string]$Disk = $env:POLIS_DEV_DISK
)

# Default VM settings
if (-not $Name) { $Name = "polis-dev" }
if (-not $Cpus) { $Cpus = 4 }
if (-not $Memory) { $Memory = "8G" }
if (-not $Disk) { $Disk = "50G" }

$UbuntuVersion = "24.04"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir

# Colors for output
function Write-Info { 
    Write-Host "[INFO] $args" -ForegroundColor Blue 
}

function Write-Success { 
    Write-Host "[OK] $args" -ForegroundColor Green 
}

function Write-Warn { 
    Write-Host "[WARN] $args" -ForegroundColor Yellow 
}

function Write-Error-Custom { 
    Write-Host "[ERROR] $args" -ForegroundColor Red 
}

function Write-Step { 
    Write-Host "[STEP] $args" -ForegroundColor Cyan 
}

function Show-Usage {
    Write-Host @"
Polis Developer Environment Setup (Windows)

Usage: .\polis-dev.ps1 <command> [options]

Commands:
  create        Create a new development VM
  start         Start the development VM
  stop          Stop the development VM
  shell         Enter the development VM shell
  delete        Delete the development VM
  status        Show VM status
  mount         Mount local polis directory (auto-mounted on create)
  unmount       Unmount local polis directory
  rebuild       Rebuild Polis from source inside VM
  fix-perms     Fix file ownership issues in mounted directory

Options:
  -Name <name>      VM name (default: polis-dev)
  -Cpus <n>         Number of CPUs (default: 4)
  -Memory <size>    Memory size (default: 8G)
  -Disk <size>      Disk size (default: 50G)

Environment Variables:
  POLIS_DEV_VM_NAME    Override default VM name
  POLIS_DEV_CPUS       Override CPU count
  POLIS_DEV_MEMORY     Override memory size
  POLIS_DEV_DISK       Override disk size

Examples:
  .\polis-dev.ps1 create                    # Create VM with defaults
  .\polis-dev.ps1 create -Cpus 8 -Memory 16G  # Create with custom resources
  .\polis-dev.ps1 shell                     # Access VM shell
  .\polis-dev.ps1 rebuild                   # Rebuild Polis in VM
  .\polis-dev.ps1 fix-perms                 # Fix file permissions
"@
}

function Test-Multipass {
    if (-not (Get-Command multipass -ErrorAction SilentlyContinue)) {
        Write-Error-Custom "Multipass is not installed."
        Write-Host ""
        Write-Host "Install Multipass:"
        Write-Host "  winget install Canonical.Multipass"
        Write-Host ""
        Write-Host "Visit: https://multipass.run/install"
        return $false
    }
    return $true
}

function Test-VMExists {
    $vms = multipass list 2>$null | Select-String "^$Name\s"
    return $null -ne $vms
}

function New-VM {
    Write-Host ""
    Write-Step "Creating Polis development VM: $Name"
    Write-Host ""
    
    if (Test-VMExists) {
        Write-Warn "VM '$Name' already exists."
        Write-Host ""
        $response = Read-Host "Delete and recreate? [y/N]"
        if ($response -ne 'y' -and $response -ne 'Y') {
            Write-Info "Aborting."
            return
        }
        Remove-VM
    }
    
    Write-Info "VM Configuration:"
    Write-Host "  Name:   $Name"
    Write-Host "  CPUs:   $Cpus"
    Write-Host "  Memory: $Memory"
    Write-Host "  Disk:   $Disk"
    Write-Host ""
    
    Write-Info "Launching VM (this takes 5-10 minutes)..."
    $cloudInitPath = Join-Path $ProjectRoot "polis-dev.yaml"
    multipass launch `
        --name $Name `
        --cpus $Cpus `
        --memory $Memory `
        --disk $Disk `
        --cloud-init $cloudInitPath `
        $UbuntuVersion
    
    Write-Info "Waiting for cloud-init to complete..."
    multipass exec $Name -- cloud-init status --wait
    
    Write-Success "VM created successfully!"
    Write-Host ""
    
    # Mount project directory
    Write-Step "Mounting local polis directory..."
    multipass mount $ProjectRoot "${Name}:/home/ubuntu/polis"
    Write-Success "Mounted $ProjectRoot -> ${Name}:/home/ubuntu/polis"
    Write-Host ""
    
    # Show next steps
    Write-Success "Development VM is ready!"
    Write-Host ""
    Write-Host "Next steps:"
    Write-Host "  1. Enter VM:        .\polis-dev.ps1 shell"
    Write-Host "  2. Configure keys:  cd ~/polis && nano .env"
    Write-Host "  3. Build & start:   .\polis-dev.ps1 rebuild"
    Write-Host ""
    Write-Host "Or manually:"
    Write-Host "  multipass shell $Name"
    Write-Host "  cd ~/polis"
    Write-Host "  ./cli/polis.sh init --local"
}

function Start-VM {
    if (-not (Test-VMExists)) {
        Write-Error-Custom "VM '$Name' does not exist. Create it first with: .\polis-dev.ps1 create"
        return
    }
    
    Write-Info "Starting VM '$Name'..."
    multipass start $Name
    Write-Success "VM started."
}

function Stop-VM {
    if (-not (Test-VMExists)) {
        Write-Error-Custom "VM '$Name' does not exist."
        return
    }
    
    Write-Info "Stopping VM '$Name'..."
    multipass stop $Name
    Write-Success "VM stopped."
}

function Remove-VM {
    if (-not (Test-VMExists)) {
        Write-Warn "VM '$Name' does not exist."
        return
    }
    
    Write-Info "Deleting VM '$Name'..."
    multipass delete $Name
    multipass purge
    Write-Success "VM deleted."
}

function Enter-Shell {
    if (-not (Test-VMExists)) {
        Write-Error-Custom "VM '$Name' does not exist. Create it first with: .\polis-dev.ps1 create"
        return
    }
    
    Write-Info "Entering VM '$Name'..."
    multipass shell $Name
}

function Show-Status {
    if (-not (Test-VMExists)) {
        Write-Warn "VM '$Name' does not exist."
        return
    }
    
    multipass info $Name
}

function Mount-Project {
    if (-not (Test-VMExists)) {
        Write-Error-Custom "VM '$Name' does not exist."
        return
    }
    
    Write-Info "Mounting $ProjectRoot -> ${Name}:/home/ubuntu/polis"
    multipass mount $ProjectRoot "${Name}:/home/ubuntu/polis"
    Write-Success "Mounted successfully."
}

function Dismount-Project {
    if (-not (Test-VMExists)) {
        Write-Error-Custom "VM '$Name' does not exist."
        return
    }
    
    Write-Info "Unmounting polis directory..."
    multipass unmount "${Name}:/home/ubuntu/polis"
    Write-Success "Unmounted successfully."
}

function Rebuild-Polis {
    if (-not (Test-VMExists)) {
        Write-Error-Custom "VM '$Name' does not exist."
        return
    }
    
    Write-Info "Rebuilding Polis from source in VM..."
    Write-Host ""
    
    multipass exec $Name -- bash -c @"
        cd ~/polis
        ./cli/polis.sh down 2>/dev/null || true
        ./cli/polis.sh init --local --no-cache
"@
    
    Write-Success "Rebuild complete!"
    Write-Host ""
    Write-Host "Get access token with: .\polis-dev.ps1 shell"
    Write-Host "Then run: ./cli/polis.sh openclaw init"
}

function Fix-Permissions {
    if (-not (Test-VMExists)) {
        Write-Error-Custom "VM '$Name' does not exist."
        return
    }
    
    Write-Info "Fixing file ownership in mounted directory..."
    Write-Host ""
    
    # Change ownership of all files in ~/polis to ubuntu:ubuntu
    multipass exec $Name -- sudo chown -R ubuntu:ubuntu /home/ubuntu/polis
    
    Write-Success "Permissions fixed!"
    Write-Host ""
    Write-Info "All files in the mounted directory are now owned by the VM user."
    Write-Info "You should be able to edit them from your host without issues."
}

# Main command dispatcher
if (-not (Test-Multipass)) {
    exit 1
}

switch ($Command) {
    "create" { New-VM }
    "start" { Start-VM }
    "stop" { Stop-VM }
    "delete" { Remove-VM }
    "shell" { Enter-Shell }
    "status" { Show-Status }
    "mount" { Mount-Project }
    "unmount" { Dismount-Project }
    "rebuild" { Rebuild-Polis }
    "fix-perms" { Fix-Permissions }
    "help" { Show-Usage }
    "--help" { Show-Usage }
    "-h" { Show-Usage }
    default {
        if ($Command) {
            Write-Error-Custom "Unknown command: $Command"
            Write-Host ""
        }
        Show-Usage
        if ($Command) { exit 1 }
    }
}
