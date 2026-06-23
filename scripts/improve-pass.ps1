<#
.SYNOPSIS
  Scheduled architecture / refactor improvement pass for PrizePicks Monster.
.DESCRIPTION
  - Headless: agent runs and reports. Never commits.
  - Starts / reuses a long-lived `opencode serve` and runs `opencode run --attach`
    against it. This works around opencode #28407 (opencode run headless returns
    "Session not found" on Windows).
  - Uses a lockfile to prevent overlapping runs.
  - Logs to reports/improve-pass/<timestamp>.log and writes a final report md.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$ProjectRoot  = 'C:\Projects\prizepicks-monster'
$LockFile     = Join-Path $ProjectRoot '.improve-pass.lock'
$LogDir       = Join-Path $ProjectRoot 'reports\improve-pass'
$Stamp        = Get-Date -Format 'yyyy-MM-dd_HH-mm-ss'
$LogFile      = Join-Path $LogDir "$Stamp.log"
$ReportFile   = Join-Path $LogDir "$Stamp.report.md"
$ServerUrl    = 'http://127.0.0.1:4096'
$ServerExe    = Join-Path $env:APPDATA 'npm\node_modules\opencode-ai\bin\opencode.exe'
$ServeLog     = Join-Path $env:TEMP 'opencode-serve.out'
$Model        = $env:IMPROVE_PASS_MODEL
if ([string]::IsNullOrWhiteSpace($Model)) { $Model = 'opencode/big-pickle' }

# --- Lockfile guard ---
if (Test-Path -LiteralPath $LockFile) {
    $existingPid = 0
    try { $existingPid = [int](Get-Content -LiteralPath $LockFile -Raw -ErrorAction SilentlyContinue) } catch {}
    $stillRunning = $false
    if ($existingPid -gt 0) {
        $proc = Get-Process -Id $existingPid -ErrorAction SilentlyContinue
        if ($proc) { $stillRunning = $true }
    }
    if ($stillRunning) {
        Write-Output "[skip] Another improve-pass is running (pid $existingPid). Exiting."
        exit 0
    } else {
        Write-Output "[info] Stale lockfile from pid $existingPid. Removing."
        Remove-Item -LiteralPath $LockFile -Force -ErrorAction SilentlyContinue
    }
}

New-Item -ItemType Directory -Path $LogDir -Force | Out-Null
[System.IO.File]::WriteAllText($LockFile, "$PID")
Write-Output "[start] pid=$PID at $(Get-Date -Format o)"
Write-Output "[workdir] $ProjectRoot"
Write-Output "[model] $Model"
Write-Output "[server] $ServerUrl"
Write-Output "[log] $LogFile"
Write-Output "[report] $ReportFile"

function Test-Server {
    try {
        $client = New-Object System.Net.Sockets.TcpClient
        $iar = $client.BeginConnect('127.0.0.1', 4096, $null, $null)
        $ok = $iar.AsyncWaitHandle.WaitOne(2000)
        $client.Close()
        return $ok
    } catch { return $false }
}

function Start-Server {
    if (-not (Test-Path -LiteralPath $ServerExe)) {
        throw "opencode.exe not found at $ServerExe"
    }
    Write-Output "[serve] starting opencode serve on port 4096"
    Start-Process -FilePath $ServerExe -ArgumentList 'serve','--port','4096' -WindowStyle Hidden `
        -RedirectStandardOutput $ServeLog -RedirectStandardError "$ServeLog.err"
    for ($i = 0; $i -lt 30; $i++) {
        Start-Sleep -Seconds 1
        if (Test-Server) { return $true }
    }
    return $false
}

try {
    Set-Location $ProjectRoot

    if (-not (Test-Server)) {
        if (-not (Start-Server)) {
            throw "opencode serve did not become ready within 30s. See $ServeLog"
        }
    }
    Write-Output "[serve] ready"

    $prompt = @"
You are running a scheduled architecture / refactor improvement pass on this repo (PrizePicks Monster).

Mandatory behavior:
1. Load and use the `improve-codebase-architecture` skill to drive the pass.
2. Read AGENTS.md and any CONTEXT.md / docs/adr/ before proposing changes; honor the project posture.
3. Only propose / make changes that genuinely improve the codebase. Skip trivial or speculative refactors.
4. Do NOT run `git commit`, `git push`, or any commit-related commands. The user reviews and commits manually.
5. Do not start long-lived services, place bets, or hit live APIs. Read-only research and code edits only.
6. Honor PrizePicks Monster rules: no Kalshi terminology, evidence-first claims, Over/Under not YES/NO.

Output a final report (concise, in markdown):
- (a) what you changed and why (file paths + brief rationale)
- (b) what is left for the human to review
- (c) any blockers / open questions

Write the final report to disk at: $ReportFile
"@

    $opencodeExe = Join-Path $env:APPDATA 'npm\node_modules\opencode-ai\bin\opencode.exe'
    $argList = @('run','--attach',$ServerUrl,'--model',$Model,'--title',"Architecture pass $Stamp",'--format','default','--dangerously-skip-permissions',$prompt)
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $opencodeExe
    $psi.Arguments = ($argList | ForEach-Object { if ($_ -match '\s') { '"' + ($_ -replace '"','\"') + '"' } else { $_ } }) -join ' '
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError  = $true
    $psi.StandardOutputEncoding = [System.Text.Encoding]::UTF8
    $psi.StandardErrorEncoding  = [System.Text.Encoding]::UTF8
    $psi.WorkingDirectory = $ProjectRoot
    $p = [System.Diagnostics.Process]::Start($psi)
    $soTask = $p.StandardOutput.ReadToEndAsync()
    $seTask = $p.StandardError.ReadToEndAsync()
    $p.WaitForExit()
    $output = $soTask.Result + $seTask.Result
    $exitCode = $p.ExitCode

    $output | Out-File -LiteralPath $LogFile -Encoding utf8
    $output | ForEach-Object { Write-Output $_ }

    Write-Output "[done] exit=$exitCode at $(Get-Date -Format o)"
    exit $exitCode
}
catch {
    Write-Output "[error] $($_.Exception.Message)"
    Write-Output "[stack] $($_.ScriptStackTrace)"
    exit 1
}
finally {
    if (Test-Path -LiteralPath $LockFile) {
        Remove-Item -LiteralPath $LockFile -Force -ErrorAction SilentlyContinue
    }
}
