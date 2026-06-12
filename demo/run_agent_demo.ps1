#requires -Version 5.1
<#
.SYNOPSIS
    Run real Copilot CLI sessions (baseline vs TokenZero-MCP-only) and write
    a live-updating JSON file the viewer page polls.

.DESCRIPTION
    For each replicate, runs one Copilot CLI session per condition:
      - baseline:   native tools only (view, bash, etc.)
      - tokenzero:  TokenZero MCP attached, native file/shell tools DENIED
    JSONL output is captured per run; metrics are parsed and aggregated
    into demo\agent_results.json after every completed run so the live
    viewer (agent_viz.html) updates in real time.

    The viewer is served over HTTP via Python's built-in http.server so
    fetch() works (browsers reject fetch from file://).

.PARAMETER Replicates
    Number of runs per condition. Default 3.

.PARAMETER Model
    Copilot CLI model id. Default gpt-5-mini.

.PARAMETER Conditions
    Which conditions to run. Default both.

.PARAMETER Port
    Local HTTP port for the viewer. Default 8765.

.PARAMETER NoServe
    Skip starting the local HTTP server (assume one is already running, or
    you only want to update the JSON).

.PARAMETER NoOpen
    Don't auto-open the browser.

.PARAMETER PerRunTimeoutSec
    Hard ceiling per Copilot CLI invocation. Default 240.
#>
[CmdletBinding()]
param(
    [int]    $Replicates       = 3,
    [string] $Model            = 'gpt-5-mini',
    [string[]] $Conditions     = @('baseline','tokenzero'),
    [int]    $Port             = 8765,
    [switch] $NoServe,
    [switch] $NoOpen,
    [int]    $PerRunTimeoutSec = 300,
    [string] $BinaryPath,
    [string] $CopilotPath
)

$ErrorActionPreference = 'Stop'
$DemoDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoDir = Split-Path -Parent $DemoDir
$RunsDir = Join-Path $DemoDir 'agent_runs'
$CacheDir = Join-Path $DemoDir '.cache'
$ResultsPath = Join-Path $DemoDir 'agent_results.json'
$McpCfgPath  = Join-Path $DemoDir 'tokenzero-mcp.json'

New-Item -ItemType Directory -Force -Path $RunsDir, $CacheDir | Out-Null

# --- resolve binaries -------------------------------------------------------
if (-not $BinaryPath) {
    $cand = @(
        Join-Path $env:USERPROFILE '.copilot\session-state\cfe34fed-2c6f-4464-a63c-f2d324670497\files\tokenzero\extracted\tokenzero-v1.0.1-x86_64-pc-windows-msvc\tokenzero.exe'
    )
    foreach ($p in $cand) { if (Test-Path $p) { $BinaryPath = $p; break } }
    if (-not $BinaryPath) {
        $cmd = Get-Command tokenzero -ErrorAction SilentlyContinue
        if ($cmd) { $BinaryPath = $cmd.Source }
    }
}
if (-not $BinaryPath -or -not (Test-Path $BinaryPath)) {
    throw "tokenzero binary not found. Pass -BinaryPath."
}
if (-not $CopilotPath) {
    $cmd = Get-Command copilot -ErrorAction SilentlyContinue
    if ($cmd) { $CopilotPath = $cmd.Source }
}
if (-not $CopilotPath) { throw "copilot CLI not found on PATH. Pass -CopilotPath." }

Write-Host "tokenzero: $BinaryPath"
Write-Host "copilot:   $CopilotPath"
Write-Host "repo:      $RepoDir"
Write-Host "runs dir:  $RunsDir"

function ConvertTo-Win32CommandLineArg {
    # Windows CommandLineToArgvW quoting rules (msvcrt-compatible).
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Value)
    if ($Value -eq '') { return '""' }
    if ($Value -notmatch '[\s"]') { return $Value }
    $sb = [System.Text.StringBuilder]::new()
    [void]$sb.Append('"')
    $bs = 0
    foreach ($ch in $Value.ToCharArray()) {
        if ($ch -eq '\') { $bs++; continue }
        if ($ch -eq '"') {
            [void]$sb.Append('\' * (2 * $bs + 1)); [void]$sb.Append('"'); $bs = 0; continue
        }
        if ($bs -gt 0) { [void]$sb.Append('\' * $bs); $bs = 0 }
        [void]$sb.Append($ch)
    }
    if ($bs -gt 0) { [void]$sb.Append('\' * (2 * $bs)) }
    [void]$sb.Append('"')
    return $sb.ToString()
}

# --- emit MCP config from template -----------------------------------------
$tplPath = Join-Path $DemoDir 'tokenzero-mcp.template.json'
$cacheFile = Join-Path $CacheDir 'agent-tokenzero.json'
$tpl = Get-Content -LiteralPath $tplPath -Raw
$cfg = $tpl `
    -replace '__TOKENZERO_BIN__', ($BinaryPath -replace '\\','\\') `
    -replace '__REPO__',          ($RepoDir    -replace '\\','\\') `
    -replace '__CACHE__',         ($cacheFile  -replace '\\','\\')
Set-Content -LiteralPath $McpCfgPath -Value $cfg -Encoding UTF8
Write-Host "wrote MCP config: $McpCfgPath"

# --- tool whitelist for TokenZero-only condition ---------------------------
$TokenZeroTools = @(
    'report_intent',
    'tokenzero-tz_read','tokenzero-tz_find','tokenzero-tz_grep','tokenzero-tz_glob',
    'tokenzero-tz_tree','tokenzero-tz_shell','tokenzero-tz_edit','tokenzero-tz_batch',
    'tokenzero-tz_ingest','tokenzero-tz_expand','tokenzero-tz_recall','tokenzero-tz_fetch',
    'tokenzero-tz_mem','tokenzero-tz_cache_pack','tokenzero-tz_rewrite','tokenzero-tz_discover',
    # plain names just in case naming convention differs:
    'tz_read','tz_find','tz_grep','tz_glob','tz_tree','tz_shell','tz_edit','tz_batch',
    'tz_ingest','tz_expand','tz_recall','tz_fetch','tz_mem','tz_cache_pack','tz_rewrite','tz_discover'
) -join ','

# Native tools to EXCLUDE in the TokenZero condition (force model onto tz_*)
$NativeDeny = @(
    'view','bash','powershell','read_powershell','str_replace_editor',
    'create','edit','grep','glob','find','read','write','run'
) -join ','

# --- agent prompt (same for both conditions) -------------------------------
$Prompt = @'
TASK: Find every place a JSON-RPC error response is constructed in the
tokenzero-mcp crate (crates/tokenzero-mcp/src/). For each, report file:line
and a short note about when it fires.

RULES (follow exactly):
- Start with a tool call IMMEDIATELY. Do not write a plan first.
- Use at most 6 tool calls.
- Final reply must be ONLY a markdown table with columns:
  | File:Line | Code | When |
- No prose. No reasoning. No "intent". Table only.
'@

# --- skeleton results file with all runs marked pending --------------------
$plan = @()
$idx = 0
foreach ($r in 1..$Replicates) {
    foreach ($c in $Conditions) {
        $idx++
        $plan += [ordered]@{
            index       = $idx
            condition   = $c
            replicate   = $r
            status      = 'pending'
            wall_ms     = $null
            api_ms      = $null
            input_tokens  = $null
            output_tokens = $null
            tool_calls    = $null
            tool_output_tokens = $null
            exit_code   = $null
            note        = ''
            jsonl_path  = ''
        }
    }
}

function Save-Results {
    param($Meta, $Runs, $StartTime)
    $arr     = @($Runs)
    $done    = @($arr | Where-Object { $_.status -eq 'done'    }).Count
    $failed  = @($arr | Where-Object { $_.status -eq 'failed'  }).Count
    $running = @($arr | Where-Object { $_.status -eq 'running' }).Count

    function Stats($cond) {
        $rows = @($arr | Where-Object { $_.condition -eq $cond -and $_.status -eq 'done' })
        if (-not $rows -or $rows.Count -eq 0) { return [ordered]@{ n = 0 } }
        function Mean($p) {
            $vals = $rows | ForEach-Object { $_.$p } | Where-Object { $_ -ne $null }
            if (-not $vals -or $vals.Count -eq 0) { return $null }
            ($vals | Measure-Object -Average).Average
        }
        function Std($p) {
            $vals = @($rows | ForEach-Object { $_.$p } | Where-Object { $_ -ne $null })
            if ($vals.Count -lt 2) { return $null }
            $m = ($vals | Measure-Object -Average).Average
            $sumsq = 0.0; foreach ($v in $vals) { $sumsq += ($v - $m) * ($v - $m) }
            [Math]::Sqrt($sumsq / ($vals.Count - 1))
        }
        [ordered]@{
            n                            = $rows.Count
            mean_tool_output_tokens      = (Mean 'tool_output_tokens')
            stddev_tool_output_tokens    = (Std  'tool_output_tokens')
            mean_tool_calls              = (Mean 'tool_calls')
            mean_wall_ms                 = (Mean 'wall_ms')
            mean_api_ms                  = (Mean 'api_ms')
            mean_input_tokens            = (Mean 'input_tokens')
            mean_output_tokens           = (Mean 'output_tokens')
        }
    }

    $elapsed = [int]([DateTime]::UtcNow - $StartTime).TotalMilliseconds
    $payload = [ordered]@{
        meta = $Meta
        totals = [ordered]@{
            done       = $done
            failed     = $failed
            running    = $running
            total      = $Runs.Count
            elapsed_ms = $elapsed
        }
        summary = [ordered]@{
            baseline  = (Stats 'baseline')
            tokenzero = (Stats 'tokenzero')
        }
        runs = $Runs
    }
    $tmp = $ResultsPath + '.tmp'
    ($payload | ConvertTo-Json -Depth 8) | Set-Content -LiteralPath $tmp -Encoding UTF8
    Move-Item -LiteralPath $tmp -Destination $ResultsPath -Force
}

$Meta = [ordered]@{
    task        = 'jsonrpc_errors'
    model       = $Model
    replicates  = $Replicates
    conditions  = $Conditions
    repo        = $RepoDir
    started_at  = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
}
$StartUtc = [DateTime]::UtcNow
Save-Results -Meta $Meta -Runs $plan -StartTime $StartUtc
Write-Host "wrote initial: $ResultsPath"

# --- ensure viewer HTML exists ---------------------------------------------
$VizPath = Join-Path $DemoDir 'agent_viz.html'
if (-not (Test-Path $VizPath)) {
    & (Join-Path $DemoDir 'build_agent_viz.ps1') | Out-Null
}

# --- start local HTTP server ----------------------------------------------
$serverProc = $null
if (-not $NoServe) {
    Write-Host "starting HTTP server on port $Port (serving $DemoDir)..."
    $py = (Get-Command python -ErrorAction SilentlyContinue).Source
    if (-not $py) { $py = (Get-Command py -ErrorAction SilentlyContinue).Source }
    if (-not $py) { throw "python not found; pass -NoServe and serve $DemoDir yourself." }
    $serverProc = Start-Process -FilePath $py `
        -ArgumentList '-u','-m','http.server',"$Port",'--bind','127.0.0.1' `
        -WorkingDirectory $DemoDir -WindowStyle Hidden -PassThru
    Start-Sleep -Milliseconds 600
    if ($serverProc.HasExited) { throw "HTTP server exited immediately (port $Port in use?)." }
    Write-Host "server PID: $($serverProc.Id)"
    if (-not $NoOpen) {
        $url = "http://127.0.0.1:$Port/agent_viz.html"
        Write-Host "opening $url"
        Start-Process $url
    }
}

# --- helper: invoke copilot and capture JSONL (streams to disk live) ------
function Invoke-CopilotRun {
    param(
        [string] $Condition,
        [int]    $RunIndex,
        [string] $JsonlPath,
        [scriptblock] $OnTick
    )
    $copilotArgs = @(
        '-p', $Prompt,
        '--output-format','json',
        '--model', $Model,
        '--no-ask-user',
        '--allow-all-paths',
        '-C', $RepoDir,
        '--log-level','error'
    )
    if ($Condition -eq 'baseline') {
        # Native tools only - no TokenZero MCP attached, no constraints.
        $copilotArgs += @('--allow-all-tools')
    } else {
        # TokenZero condition: attach the MCP server, then EXCLUDE every
        # native shell/file tool so the model has no choice but to use the
        # tz_* tools from the tokenzero MCP server. We use --excluded-tools
        # (a hard filter on the model's tool list) rather than --deny-tool
        # (which only affects permission prompts, not visibility).
        $copilotArgs += @('--additional-mcp-config', "@$McpCfgPath")
        $copilotArgs += @('--allow-all-tools')
        $copilotArgs += @('--excluded-tools', $NativeDeny)
    }
    # Disable the user's pre-configured heavy MCP servers so the agent isn't
    # blocked on tool-listing handshakes that have nothing to do with the task.
    # (Identified empirically: with these enabled, gpt-5-mini hangs after the
    #  intent message and never issues tool calls.)
    $copilotArgs += @('--disable-builtin-mcps')
    foreach ($s in 'Azure','icm-mcp-prod','github') {
        $copilotArgs += @('--disable-mcp-server', $s)
    }

    $stderrPath = "$JsonlPath.err"
    if (Test-Path $JsonlPath)  { Remove-Item -LiteralPath $JsonlPath -Force }
    if (Test-Path $stderrPath) { Remove-Item -LiteralPath $stderrPath -Force }

    # Build a single, Win32-quoted command line so multi-word/multi-line args
    # (notably the prompt) survive intact.
    $quotedArgs = ($copilotArgs | ForEach-Object { ConvertTo-Win32CommandLineArg $_ }) -join ' '

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $proc = Start-Process -FilePath $CopilotPath -ArgumentList $quotedArgs `
        -WorkingDirectory $RepoDir `
        -RedirectStandardOutput $JsonlPath `
        -RedirectStandardError  $stderrPath `
        -NoNewWindow -PassThru

    # Poll every 2s so the live viewer updates mid-run
    while (-not $proc.WaitForExit(2000)) {
        if ($OnTick) {
            try { & $OnTick $sw.ElapsedMilliseconds $JsonlPath } catch {}
        }
        if ($sw.Elapsed.TotalSeconds -gt $PerRunTimeoutSec) {
            try { $proc.Kill($true) } catch {}
            $proc.WaitForExit()
            $sw.Stop()
            $stderrTo = if (Test-Path $stderrPath) { Get-Content -LiteralPath $stderrPath -Raw } else { '' }
            return @{
                exit_code = -1
                wall_ms   = $sw.ElapsedMilliseconds
                timed_out = $true
                stderr    = $stderrTo
            }
        }
    }
    $sw.Stop()
    $stderr = if (Test-Path $stderrPath) { Get-Content -LiteralPath $stderrPath -Raw } else { '' }
    @{
        exit_code = $proc.ExitCode
        wall_ms   = $sw.ElapsedMilliseconds
        timed_out = $false
        stderr    = $stderr
    }
}

# --- helper: lightweight mid-run progress (lines, tool calls so far) ------
function Get-MidRunProgress {
    param([string] $JsonlPath)
    if (-not (Test-Path $JsonlPath)) { return $null }
    $lines = 0; $tools = 0; $msgs = 0
    try {
        $reader = [System.IO.StreamReader]::new(
            [System.IO.File]::Open($JsonlPath,'Open','Read','ReadWrite'),
            [System.Text.Encoding]::UTF8)
        try {
            while (-not $reader.EndOfStream) {
                $line = $reader.ReadLine()
                if (-not $line) { continue }
                $lines++
                if ($line -match '"type"\s*:\s*"tool\.execution_start"')   { $tools++ }
                elseif ($line -match '"type"\s*:\s*"assistant\.message"')  { $msgs++ }
            }
        } finally { $reader.Dispose() }
    } catch { return $null }
    @{ lines = $lines; tool_calls = $tools; messages = $msgs }
}

# --- helper: parse JSONL ---------------------------------------------------
function Parse-RunMetrics {
    param([string] $JsonlPath)
    $events = @()
    foreach ($line in (Get-Content -LiteralPath $JsonlPath)) {
        if (-not $line) { continue }
        try { $events += ($line | ConvertFrom-Json) } catch {}
    }
    $assistantMsgs = $events | Where-Object { $_.type -eq 'assistant.message' }
    $outputTok = ($assistantMsgs | ForEach-Object {
        if ($_.data.outputTokens) { [int]$_.data.outputTokens } else { 0 }
    } | Measure-Object -Sum).Sum
    if (-not $outputTok) { $outputTok = 0 }

    $toolCompletes = $events | Where-Object { $_.type -eq 'tool.execution_complete' }
    $toolCalls = $toolCompletes.Count

    # Sum tool output character length as proxy; then ingest each through tokenzero
    $toolBlob = New-Object System.Text.StringBuilder
    foreach ($e in $toolCompletes) {
        $r = $e.data.result
        if ($null -eq $r) { continue }
        if ($r -is [string]) {
            [void]$toolBlob.Append($r); [void]$toolBlob.Append("`n")
            continue
        }
        # Tool results may have:
        #   - result.content as a string (Copilot CLI native tools)
        #   - result.content as an array of {text:...} (MCP-style)
        #   - result.detailedContent (Copilot's verbose view payload)
        #   - result.output (rare)
        if ($r.content -is [string]) {
            [void]$toolBlob.Append($r.content); [void]$toolBlob.Append("`n")
        } elseif ($r.content) {
            foreach ($c in $r.content) {
                if ($c.text)               { [void]$toolBlob.Append([string]$c.text); [void]$toolBlob.Append("`n") }
                elseif ($c -is [string])   { [void]$toolBlob.Append($c); [void]$toolBlob.Append("`n") }
            }
        }
        if ($r.detailedContent -is [string]) {
            [void]$toolBlob.Append($r.detailedContent); [void]$toolBlob.Append("`n")
        } elseif ($r.detailedContent) {
            [void]$toolBlob.Append(($r.detailedContent | ConvertTo-Json -Depth 6 -Compress)); [void]$toolBlob.Append("`n")
        }
        if ($r.output) { [void]$toolBlob.Append([string]$r.output); [void]$toolBlob.Append("`n") }
    }
    $toolText = $toolBlob.ToString()
    $toolOutTok = 0
    if ($toolText.Length -gt 0) {
        # send to tokenzero ingest --stdin --json
        $cachePath = Join-Path $CacheDir ('ingest-' + [Guid]::NewGuid().ToString('N') + '.json')
        try {
            $psi = New-Object System.Diagnostics.ProcessStartInfo
            $psi.FileName = $BinaryPath
            foreach ($a in @('ingest','--stdin','--json','--cache-path',$cachePath)) { $psi.ArgumentList.Add($a) }
            $psi.RedirectStandardInput  = $true
            $psi.RedirectStandardOutput = $true
            $psi.RedirectStandardError  = $true
            $psi.UseShellExecute = $false
            $psi.CreateNoWindow  = $true
            $proc = [System.Diagnostics.Process]::Start($psi)
            $writer = $proc.StandardInput
            $writer.Write($toolText); $writer.Close()
            $out = $proc.StandardOutput.ReadToEnd()
            $proc.WaitForExit()
            if ($out) {
                try {
                    $j = $out | ConvertFrom-Json
                    if ($null -ne $j.accounting.raw_tokens) { $toolOutTok = [int]$j.accounting.raw_tokens }
                    elseif ($null -ne $j.tokens)            { $toolOutTok = [int]$j.tokens }
                    elseif ($null -ne $j.token_count)       { $toolOutTok = [int]$j.token_count }
                } catch {
                    $toolOutTok = [int]($toolText.Length / 4)
                }
            }
        } catch {
            $toolOutTok = [int]($toolText.Length / 4)
        }
    }

    $result = $events | Where-Object { $_.type -eq 'result' } | Select-Object -Last 1
    $apiMs = $null; $sessionMs = $null; $premium = $null
    if ($result) {
        # The `result` event is flat (no .data wrapper).
        $u = $result.usage
        if (-not $u -and $result.data) { $u = $result.data.usage }
        if ($u) {
            if ($null -ne $u.totalApiDurationMs) { $apiMs     = [int]$u.totalApiDurationMs }
            if ($null -ne $u.sessionDurationMs) { $sessionMs = [int]$u.sessionDurationMs }
            if ($null -ne $u.premiumRequests)   { $premium   = [int]$u.premiumRequests }
        }
    }

    [ordered]@{
        output_tokens       = $outputTok
        input_tokens        = $null   # not exposed by Copilot CLI JSONL
        tool_calls          = $toolCalls
        tool_output_tokens  = $toolOutTok
        api_ms              = $apiMs
        session_ms          = $sessionMs
        premium_requests    = $premium
    }
}

# --- main loop -------------------------------------------------------------
Write-Host ""
Write-Host "starting $($plan.Count) runs..."
foreach ($run in $plan) {
    $tag = "{0}-r{1}" -f $run.condition, $run.replicate
    $jsonl = Join-Path $RunsDir "$tag.jsonl"
    $run.jsonl_path = $jsonl
    $run.status = 'running'
    Save-Results -Meta $Meta -Runs $plan -StartTime $StartUtc
    Write-Host "  [$($run.index)/$($plan.Count)] $tag ... " -NoNewline

    $r = Invoke-CopilotRun -Condition $run.condition -RunIndex $run.index -JsonlPath $jsonl `
        -OnTick {
            param($elapsedMs, $jp)
            $p = Get-MidRunProgress -JsonlPath $jp
            if ($p) {
                $run.note = "live: $($p.lines) events, $($p.tool_calls) tool calls, $($p.messages) msgs ($([Math]::Round($elapsedMs/1000))s)"
                $run.wall_ms = [int]$elapsedMs
                $run.tool_calls = $p.tool_calls
                Save-Results -Meta $Meta -Runs $plan -StartTime $StartUtc
            }
        }
    $run.wall_ms   = [int]$r.wall_ms
    $run.exit_code = $r.exit_code
    $wallSec = [Math]::Round($r.wall_ms / 1000, 1)
    if ($r.timed_out) {
        $run.status = 'failed'
        $run.note = "timeout @ $PerRunTimeoutSec s"
        Write-Host "TIMEOUT ($wallSec s)"
        # Even on timeout, try to extract whatever metrics we have
        try {
            $m = Parse-RunMetrics -JsonlPath $jsonl
            foreach ($k in $m.Keys) { if ($null -ne $m[$k]) { $run[$k] = $m[$k] } }
            $run.note += " (partial: $($run.tool_calls) tool calls, $($run.output_tokens) out tok)"
        } catch {}
    } elseif ($r.exit_code -ne 0) {
        $run.status = 'failed'
        $shortErr = ($r.stderr -split "`n" | Where-Object { $_ } | Select-Object -First 1)
        if (-not $shortErr) { $shortErr = '(no stderr)' }
        $run.note = "exit=$($r.exit_code) $shortErr"
        Write-Host "FAILED exit=$($r.exit_code) ($wallSec s)"
    } else {
        try {
            $m = Parse-RunMetrics -JsonlPath $jsonl
            foreach ($k in $m.Keys) { $run[$k] = $m[$k] }
            $run.status = 'done'
            $apiSec = if ($run.api_ms) { [Math]::Round($run.api_ms / 1000, 1) } else { 'n/a' }
            Write-Host "OK  wall=${wallSec}s api=${apiSec}s tools=$($run.tool_calls) toolTok=$($run.tool_output_tokens) outTok=$($run.output_tokens)"
        } catch {
            $run.status = 'failed'
            $run.note = "parse error: $($_.Exception.Message)"
            Write-Host "PARSE-FAIL: $($_.Exception.Message)"
        }
    }
    Save-Results -Meta $Meta -Runs $plan -StartTime $StartUtc
}

Write-Host ""
Write-Host "all runs complete. results: $ResultsPath"
if (-not $NoServe -and $serverProc -and -not $serverProc.HasExited) {
    Write-Host "HTTP server still running on port $Port (PID $($serverProc.Id))."
    Write-Host "Stop with: Stop-Process -Id $($serverProc.Id)"
}
