# Build and package CriticalMass for Windows (itch.io release)
# Run from repo root: ./scripts/build_release_windows.ps1

# todo: convert this to a rust script that encrypts all assets

param(
    [string]$Version = "0.1.0"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path $PSScriptRoot -Parent
$DistDir = "$RepoRoot\dist\windows"

Push-Location $RepoRoot
make build-release
if ($LASTEXITCODE -ne 0) { throw "make build-release failed" }
Pop-Location

if (Test-Path $DistDir) { Remove-Item $DistDir -Recurse -Force }
New-Item -ItemType Directory -Path $DistDir | Out-Null

Write-Host "Copying binaries..."
Copy-Item "$RepoRoot\target\release\client.exe"     "$DistDir\client.exe"
Copy-Item "$RepoRoot\target\release\gameserver.exe" "$DistDir\gameserver.exe"

Write-Host "Copying runtime libraries..."
foreach ($runtimeDll in @("steam_api64.dll", "fmod.dll", "fmodstudio.dll")) {
    $src = "$RepoRoot\target\release\$runtimeDll"
    if (-not (Test-Path $src)) { throw "$runtimeDll not found in target/release" }
    Copy-Item $src "$DistDir\$runtimeDll"
}

Write-Host "Copying assets..."
$AssetsDir = "$RepoRoot\assets"
New-Item -ItemType Directory -Path "$DistDir\assets" -Force | Out-Null
Copy-Item "$AssetsDir\*" "$DistDir\assets" -Recurse

Write-Host "Zipping..."
$ZipPath = "$RepoRoot\dist\criticalmass-windows-$Version.zip"
if (Test-Path $ZipPath) { Remove-Item $ZipPath }
Compress-Archive -Path "$DistDir\*" -DestinationPath $ZipPath

Write-Host "Done: $ZipPath"
