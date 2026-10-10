<#
.SYNOPSIS
    Drives a Pumpkin server from its console: feeds it a file of commands
    followed by `stop`, then waits for the process to actually exit.

.DESCRIPTION
    Minecraft cannot be played here, so the server console is the test harness.
    The console sender always passes permission checks on the host
    (`crates/pumpkin/src/command/mod.rs:225` returns `true` for `Console` and
    `Rcon` before any registry lookup), so every command of every plugin is
    reachable with no op and no client.

    There is deliberately no Start-Sleep in this script. The only wait is
    `WaitForExit`, which blocks until the server is really gone; if it overruns the
    timeout the process is killed and the run fails loudly rather than hanging.
    stdout and stderr go straight to files, so nothing depends on a pipe being
    drained and the server cannot block on a full pipe buffer.

    The input file is written **without a byte order mark**. `Set-Content
    -Encoding utf8` on Windows PowerShell 5.1 emits a BOM, which reaches the
    command parser as part of the first command and turns it into an unknown
    command. That cost an afternoon once.

.PARAMETER ServerDir
    The scratch server directory, the one holding pumpkin.exe.

.PARAMETER Commands
    Path to a file of console commands, one per line. `stop` is appended.

.PARAMETER TimeoutSeconds
    How long to wait before giving up.

.PARAMETER OutFile
    Where to write the captured output. Defaults to next to the command file.

.EXAMPLE
    .\run-server.ps1 -ServerDir C:\pumpkin-test -Commands .\cmds.txt
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $ServerDir,

    [Parameter(Mandatory = $true)]
    [string] $Commands,

    [Parameter()]
    [int] $TimeoutSeconds = 300,

    [Parameter()]
    [string] $OutFile
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$exe = Join-Path $ServerDir 'pumpkin.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw "no pumpkin.exe in $ServerDir" }
if (-not (Test-Path -LiteralPath $Commands)) { throw "no command file at $Commands" }

if (-not $OutFile) {
    $OutFile = [System.IO.Path]::ChangeExtension($Commands, '.out.txt')
}
$errFile = [System.IO.Path]::ChangeExtension($OutFile, '.err.txt')
$stdinFile = [System.IO.Path]::ChangeExtension($OutFile, '.stdin.txt')

# Read the caller's commands, then shut the server down ourselves so a run always
# terminates even when the commands hang the server.
$lines = @(Get-Content -LiteralPath $Commands | Where-Object { $_.Trim() -ne '' })
$lines += 'stop'

$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllLines($stdinFile, $lines, $utf8NoBom)

foreach ($f in @($OutFile, $errFile)) {
    if (Test-Path -LiteralPath $f) { Remove-Item -LiteralPath $f -Force }
}

$proc = $null
try {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $proc = Start-Process -FilePath $exe `
                          -WorkingDirectory $ServerDir `
                          -RedirectStandardInput $stdinFile `
                          -RedirectStandardOutput $OutFile `
                          -RedirectStandardError $errFile `
                          -NoNewWindow -PassThru

    # Touching Handle keeps .NET from dropping the process handle, which is what
    # makes ExitCode readable afterwards instead of null.
    $null = $proc.Handle

    $exited = $proc.WaitForExit($TimeoutSeconds * 1000)
    $sw.Stop()

    if (-not $exited) {
        try { $proc.Kill() } catch { }
        throw "the server did not stop within $TimeoutSeconds s; killed it. See $OutFile"
    }

    $proc.Refresh()
    $code = $proc.ExitCode

    $captured = @()
    foreach ($f in @($OutFile, $errFile)) {
        if (Test-Path -LiteralPath $f) {
            $captured += (Get-Content -LiteralPath $f -Raw -ErrorAction SilentlyContinue)
        }
    }
    # The logger writes ANSI colour, which makes the log unreadable and
    # ungreppable.
    $text = ($captured -join '') -replace "\x1b\[[0-9;]*[A-Za-z]", ''
    [System.IO.File]::WriteAllText($OutFile, $text, $utf8NoBom)

    Write-Output $text
    Write-Output "--- exit code $code after $([math]::Round($sw.Elapsed.TotalSeconds, 1))s ---"
    exit $code
}
finally {
    if ($null -ne $proc -and -not $proc.HasExited) { try { $proc.Kill() } catch { } }
}
