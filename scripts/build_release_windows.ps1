# Build and package CriticalMass for Windows (itch.io release)
# Run from repo root: ./scripts/build_release_windows.ps1

param(
    [string]$Version = "0.1.0"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path $PSScriptRoot -Parent
$DistDir = "$RepoRoot\dist\windows"

Write-Host "Building release binaries..."
Push-Location $RepoRoot
cargo build --release --bin client --bin gameserver
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
Pop-Location

if (Test-Path $DistDir) { Remove-Item $DistDir -Recurse -Force }
New-Item -ItemType Directory -Path $DistDir | Out-Null

Write-Host "Copying binaries..."
Copy-Item "$RepoRoot\target\release\client.exe"     "$DistDir\client.exe"
Copy-Item "$RepoRoot\target\release\gameserver.exe" "$DistDir\gameserver.exe"

Write-Host "Copying steam_api64.dll from build output..."
$SteamDll = Get-ChildItem "$RepoRoot\target\release\build\steamworks-sys-*\out\steam_api64.dll" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($SteamDll) {
    Copy-Item $SteamDll.FullName "$DistDir\steam_api64.dll"
} else {
    throw "steam_api64.dll not found in build output - was the client built?"
}

Write-Host "Copying assets (excluding blender sources)..."
$AssetsDir = "$RepoRoot\assets"
Get-ChildItem $AssetsDir -Directory | Where-Object { $_.Name -ne "blender" } | ForEach-Object {
    Copy-Item $_.FullName "$DistDir\assets\$($_.Name)" -Recurse
}

Write-Host "Zipping..."
$ZipPath = "$RepoRoot\dist\criticalmass-windows-$Version.zip"
if (Test-Path $ZipPath) { Remove-Item $ZipPath }
Compress-Archive -Path "$DistDir\*" -DestinationPath $ZipPath

Write-Host "Done: $ZipPath"
