# Run a local multiplayer test with the Rust network emulator on Windows.
# Run from repo root: ./scripts/test_multiplayer_windows.ps1

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path $PSScriptRoot -Parent
$TargetDir = Join-Path $RepoRoot "target\debug"
$ClientWorkingDir = Join-Path $RepoRoot "client"
$GameServerWorkingDir = Join-Path $RepoRoot "gameserver"
$GameServerExe = Join-Path $TargetDir "gameserver.exe"
$ClientExe = Join-Path $TargetDir "client.exe"
$EmulatorExe = Join-Path $TargetDir "network_emulator.exe"

function Stop-RepoProcess {
    param(
        [string]$Name,
        [string]$Path,
        $ProcessRef
    )

    if ($ProcessRef -and -not $ProcessRef.HasExited) {
        try {
            Stop-Process -Id $ProcessRef.Id -Force -ErrorAction Stop
        } catch {
        }
    }

    Get-Process -Name $Name -ErrorAction SilentlyContinue | ForEach-Object {
        try {
            if (-not $_.HasExited -and ((-not $_.Path) -or $_.Path -eq $Path)) {
                Stop-Process -Id $_.Id -Force -ErrorAction Stop
            }
        } catch {
        }
    }
}

function Cleanup {
    param($ClientProcess, $EmulatorProcess, $GameServerProcess)

    Stop-RepoProcess -Name "client" -Path $ClientExe -ProcessRef $ClientProcess
    Stop-RepoProcess -Name "network_emulator" -Path $EmulatorExe -ProcessRef $EmulatorProcess
    Stop-RepoProcess -Name "gameserver" -Path $GameServerExe -ProcessRef $GameServerProcess
}

trap [System.Management.Automation.PipelineStoppedException] {
    Cleanup -ClientProcess $script:ClientProcess -EmulatorProcess $script:EmulatorProcess -GameServerProcess $script:GameServerProcess
    break
}

Push-Location $RepoRoot
try {
    cargo build -p gameserver
    if ($LASTEXITCODE -ne 0) { throw "cargo build -p gameserver failed" }
    cargo build -p client
    if ($LASTEXITCODE -ne 0) { throw "cargo build -p client failed" }
    cargo build -p network_emulator
    if ($LASTEXITCODE -ne 0) { throw "cargo build -p network_emulator failed" }

    $script:GameServerProcess = Start-Process -FilePath $GameServerExe -WorkingDirectory $GameServerWorkingDir -ArgumentList @(
        "--gametype", "../assets/gametypes/default.lua"
    ) -PassThru
    $script:EmulatorProcess = Start-Process -FilePath $EmulatorExe -ArgumentList @(
        "--listen", "127.0.0.1:42069",
        "--server", "127.0.0.1:42070",
        "--loss", "0.05",
        "--mindelay", "40",
        "--maxdelay", "80",
        "--seed", "123"
    ) -PassThru

    Start-Sleep -Seconds 1

    $script:ClientProcess = Start-Process -FilePath $ClientExe -WorkingDirectory $ClientWorkingDir -ArgumentList @("--server", "127.0.0.1:42069") -PassThru

    while ($true) {
        if ($script:ClientProcess.HasExited) { break }
        Start-Sleep -Milliseconds 250
        $script:ClientProcess.Refresh()
        $script:GameServerProcess.Refresh()
        $script:EmulatorProcess.Refresh()
    }
}
finally {
    Cleanup -ClientProcess $script:ClientProcess -EmulatorProcess $script:EmulatorProcess -GameServerProcess $script:GameServerProcess
    Pop-Location
}
