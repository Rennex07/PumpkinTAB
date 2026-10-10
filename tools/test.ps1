<#
.SYNOPSIS
    Builds the plugin, drops it into a server, and drives that server from the
    console with a file of commands.

.DESCRIPTION
    The whole loop in one step, because the three halves have to agree: a build
    against the wrong WIT loads on nothing, and a server without the plugin on
    disk proves nothing.

.PARAMETER ServerDir
    The scratch server directory, the one holding pumpkin.exe.

.PARAMETER Commands
    Path to a file of console commands, one per line. `stop` is appended.

.PARAMETER Fresh
    Delete the plugin's data folder first, so the run starts from the shipped
    defaults rather than whatever the last run left behind.

.PARAMETER SkipBuild
    Deploy the existing artifact without rebuilding. Only sensible when the point
    is to test a downloaded release rather than the local build.

.EXAMPLE
    .\tools\test.ps1 -ServerDir C:\pumpkin-test -Commands .\tools\smoke.txt
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $ServerDir,

    [Parameter(Mandatory = $true)]
    [string] $Commands,

    [Parameter()]
    [switch] $Fresh,

    [Parameter()]
    [switch] $SkipBuild
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
$artifact = Join-Path $repoRoot 'target\wasm32-wasip2\release\pumpkin_tab.wasm'
$target = Join-Path $ServerDir 'plugins\pumpkin_tab.wasm'
$dataFolder = Join-Path $ServerDir 'plugins\data\PumpkinTAB'

if (-not $SkipBuild) {
    Write-Host '==> cargo build --release --target wasm32-wasip2'
    Push-Location $repoRoot
    try {
        & cargo build --release --target wasm32-wasip2
        if ($LASTEXITCODE -ne 0) { throw "the build failed with $LASTEXITCODE" }
    }
    finally { Pop-Location }
}

if (-not (Test-Path -LiteralPath $artifact)) {
    throw "no artifact at $artifact"
}

if ($Fresh -and (Test-Path -LiteralPath $dataFolder)) {
    Write-Host "==> removing $dataFolder"
    Remove-Item -Recurse -Force $dataFolder
}

Write-Host "==> deploying $artifact"
New-Item -ItemType Directory -Force -Path (Join-Path $ServerDir 'plugins') | Out-Null
Copy-Item -LiteralPath $artifact -Destination $target -Force

Write-Host '==> running the server'
& (Join-Path $PSScriptRoot 'run-server.ps1') -ServerDir $ServerDir -Commands $Commands
exit $LASTEXITCODE
