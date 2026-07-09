Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path $PSScriptRoot -Parent
$DistDir = "$RepoRoot\dist"
$OldKey = $env:CM_ASSET_KEY

Push-Location $RepoRoot
try {
    if (Test-Path $DistDir) { Remove-Item $DistDir -Recurse -Force }
    $Bytes = New-Object byte[] 32
    [System.Security.Cryptography.RandomNumberGenerator]::Fill($Bytes)
    $Key = -join ($Bytes | ForEach-Object { $_.ToString("x2") })

    cargo run -p pack_assets --release -- --key $Key --dist dist
    if ($LASTEXITCODE -ne 0) { throw "pack_assets failed" }
    $env:CM_ASSET_KEY = $Key
    cargo build -p client --release
    if ($LASTEXITCODE -ne 0) { throw "client build failed" }
    cargo build -p gameserver --release
    if ($LASTEXITCODE -ne 0) { throw "gameserver build failed" }

    Copy-Item target\release\client.exe "$DistDir\client.exe"
    Copy-Item target\release\gameserver.exe "$DistDir\gameserver.exe"
    foreach ($dll in @("steam_api64.dll", "fmod.dll", "fmodstudio.dll")) {
        Copy-Item "target\release\$dll" "$DistDir\$dll"
    }
    Write-Host "packaged dist"
} finally {
    if ($null -eq $OldKey) {
        Remove-Item Env:\CM_ASSET_KEY -ErrorAction SilentlyContinue
    } else {
        $env:CM_ASSET_KEY = $OldKey
    }
    Pop-Location
}
