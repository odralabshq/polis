# check-tools-windows.ps1 — Verify all prerequisites for building Polis on Windows
$ErrorActionPreference = "Stop"

$missing = @()

$tools = @{
    "just"       = "Task runner — winget install Casey.Just"
    "docker"     = "Docker Desktop — winget install Docker.DockerDesktop"
    "packer"     = "Packer — winget install HashiCorp.Packer"
    "cargo"      = "Rust toolchain — https://rustup.rs"
    "shellcheck" = "Shell linter — winget install koalaman.shellcheck"
    "hadolint"   = "Dockerfile linter — see install-tools-windows.ps1"
    "zipsign"    = "Artifact signing — cargo install zipsign"
}

foreach ($cmd in $tools.Keys) {
    if (Get-Command $cmd -ErrorAction SilentlyContinue) {
        $ver = try { & $cmd --version 2>&1 | Select-Object -First 1 } catch { "?" }
        Write-Host ("  [OK] {0,-15} {1}" -f $cmd, $ver) -ForegroundColor Green
    }
    else {
        Write-Host ("  [X]  {0,-15} {1}" -f $cmd, $tools[$cmd]) -ForegroundColor Red
        $missing += $cmd
    }
}

# oscdimg (Windows ADK)
$oscdimg = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10" -Recurse -Filter "oscdimg.exe" `
    -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match "amd64" } | Select-Object -First 1
if ($oscdimg) {
    Write-Host ("  [OK] {0,-15} {1}" -f "oscdimg", $oscdimg.FullName) -ForegroundColor Green
}
else {
    Write-Host ("  [X]  {0,-15} {1}" -f "oscdimg", "Windows ADK — winget install Microsoft.WindowsADK") -ForegroundColor Red
    $missing += "oscdimg"
}

# Hyper-V feature
$hv = (Get-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All -ErrorAction SilentlyContinue).State
if ($hv -eq "Enabled") {
    Write-Host ("  [OK] {0,-15}" -f "Hyper-V") -ForegroundColor Green
}
else {
    Write-Host ("  [X]  {0,-15} Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V-All" -f "Hyper-V") -ForegroundColor Red
    $missing += "Hyper-V"
}

# Hyper-V Administrators group
$hvAdmins = (Get-LocalGroupMember "Hyper-V Administrators" -ErrorAction SilentlyContinue).Name
$inGroup = $hvAdmins | Where-Object { $_ -match $env:USERNAME }
if ($inGroup) {
    Write-Host ("  [OK] {0,-15} member of Hyper-V Administrators" -f "HV-Admin") -ForegroundColor Green
}
else {
    Write-Host ("  [!]  {0,-15} Not in Hyper-V Administrators — run install-tools-windows" -f "HV-Admin") -ForegroundColor Yellow
}

Write-Host ""
if ($missing.Count -eq 0) {
    Write-Host "All required tools are present. Ready to run: just build-windows" -ForegroundColor Cyan
    exit 0
}
else {
    Write-Host "Missing $($missing.Count) tool(s). Run: just install-tools-windows" -ForegroundColor Red
    exit 1
}
