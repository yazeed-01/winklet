# Builds Winklet, installs it to %LOCALAPPDATA%\Winklet and (optionally) starts it with Windows.
# Usage:  powershell -ExecutionPolicy Bypass -File install.ps1 [-NoAutostart]
param([switch]$NoAutostart)
$ErrorActionPreference = "Stop"

cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$dest = Join-Path $env:LOCALAPPDATA "Winklet"
New-Item -ItemType Directory -Force $dest | Out-Null
Get-Process winklet -ErrorAction SilentlyContinue | Stop-Process -Force
Copy-Item "$PSScriptRoot\target\release\winklet.exe" "$dest\winklet.exe" -Force

$run = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
if ($NoAutostart) {
    Remove-ItemProperty -Path $run -Name "Winklet" -ErrorAction SilentlyContinue
} else {
    Set-ItemProperty -Path $run -Name "Winklet" -Value "`"$dest\winklet.exe`""
}

# clean up the pre-rename (DyIs) install
Remove-ItemProperty -Path $run -Name "DyIs" -ErrorAction SilentlyContinue
Get-Process dyis -ErrorAction SilentlyContinue | Stop-Process -Force

Start-Process "$dest\winklet.exe"
Write-Host "Winklet installed to $dest" -ForegroundColor Green
