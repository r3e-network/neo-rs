<#
.SYNOPSIS
    Task 57 post-replay automation: wait for the optimized MainNet replay to
    finish, verify its terminal state with the read-only state-root sweep, and
    produce the control-vs-optimized (A/B) throughput report.

.DESCRIPTION
    Read-only with respect to the node and its database:
      * never starts a neo-node process,
      * never writes into the replay database directory,
      * the only thing executed against the database is state-root-sweep.exe,
        which opens RocksDB with DB::open_for_read_only.

    Pipeline
      1. wait for the replay PID to exit (30 s poll, prints the last log line),
      2. validate the "acc import summary" (final_height / imported),
      3. run state-root-sweep.exe (release, falling back to debug),
      4. build the A/B comparison report from both replay logs
         (cumulative + trailing-100K window rates at matched heights),
      5. print the report to stdout and save it as Markdown.

    Idempotent: if the process already exited the wait step is skipped; a
    partial or failed replay is reported instead of being silently ignored.

.PARAMETER ReplayPid
    PID of the neo-node replay to monitor (default 45700).

.PARAMETER SkipWait
    Do not poll; assume the replay has finished (report regeneration).

.PARAMETER SkipSweep
    Do not execute the sweep; reuse result.json when it exists.

.PARAMETER AllowIncomplete
    Generate the report from a partial log (dry run) instead of failing when
    the terminal "acc import summary" is missing or inconsistent.
#>
[CmdletBinding()]
param(
    [int]$ReplayPid = 45700,
    [int]$PollSeconds = 30,
    [double]$MaxWaitHours = 12,
    [switch]$SkipWait,
    [switch]$SkipSweep,
    [switch]$AllowIncomplete,

    [string]$ReplayLog    = 'd:\Git\neo-rs\logs\mainnet-ab-optimized.stderr.log',
    [string]$ControlLog   = 'd:\Git\neo-rs\logs\mainnet-replay-fresh.log',
    [string]$DbPath       = 'd:\Git\neo-rs\data\mainnet-ab-optimized',
    [string]$Reference    = 'd:\Git\neo-rs\data\reference_stateroots.jsonl',
    [string]$ReportPath   = 'd:\Git\neo-rs\reports\ab-comparison-report.md',
    [string]$SweepOutput  = 'd:\Git\neo-rs\reports\mainnet-ab-optimized-sweep\result.json',
    [string]$TempDir      = 'D:\t57-temp',
    [string]$ReleaseSweep = 'd:\Git\neo-rs\.target-t59\release\state-root-sweep.exe',
    [string]$DebugSweep   = 'd:\Git\neo-rs\.target-t59\debug\state-root-sweep.exe',
    [string]$SweepLog     = 'd:\Git\neo-rs\logs\post-replay-verify-sweep.out.log',
    [string]$StateLog     = 'd:\Git\neo-rs\logs\post-replay-verify.log',
    [int]$ExpectedFinalHeight = 13141250,
    [long]$WindowBlocks   = 100000
)

$ErrorActionPreference = 'Stop'

$script:Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$script:Inv       = [System.Globalization.CultureInfo]::InvariantCulture
$script:Stopwatch = [System.Diagnostics.Stopwatch]::StartNew()

# --------------------------------------------------------------------------
# logging / io helpers
# --------------------------------------------------------------------------

function Write-Log {
    param([string]$Message, [string]$Level = 'INFO')
    $stamp = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
    $line  = '[' + $stamp + '] [' + $Level + '] ' + $Message
    Write-Host $line
    try {
        $parent = Split-Path -Parent $StateLog
        if ($parent -and -not (Test-Path -LiteralPath $parent)) {
            New-Item -ItemType Directory -Path $parent -Force | Out-Null
        }
        [System.IO.File]::AppendAllText($StateLog, $line + [Environment]::NewLine, $script:Utf8NoBom)
    } catch {
        # A logging failure must never abort the verification pipeline.
    }
}

# Read a file that another process keeps open for append (the replay log).
function Read-SharedText {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    $share = [System.IO.FileShare]([int][System.IO.FileShare]::ReadWrite -bor [int][System.IO.FileShare]::Delete)
    $stream = $null
    $reader = $null
    try {
        $stream = New-Object System.IO.FileStream($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, $share)
        $reader = New-Object System.IO.StreamReader($stream, [System.Text.Encoding]::UTF8)
        return $reader.ReadToEnd()
    } finally {
        if ($reader) { $reader.Dispose() }
        if ($stream) { $stream.Dispose() }
    }
}

function ConvertTo-UtcTime {
    param([string]$Text)
    $styles = [System.Globalization.DateTimeStyles]::AdjustToUniversal -bor [System.Globalization.DateTimeStyles]::AssumeUniversal
    return [datetime]::Parse($Text, $script:Inv, $styles)
}

function Format-Number {
    param($Value, [int]$Digits = 0)
    if ($null -eq $Value) { return 'n/a' }
    $pattern = '{0:N' + $Digits + '}'
    return [string]::Format($script:Inv, $pattern, [double]$Value)
}

function Format-Duration {
    param($Seconds)
    if ($null -eq $Seconds) { return 'n/a' }
    $span = [timespan]::FromSeconds([double]$Seconds)
    return [string]::Format($script:Inv, '{0:hh\:mm\:ss} ({1:N1} h)', $span, ([double]$Seconds / 3600.0))
}

function Format-HeightLabel {
    param([long]$Height)
    if ($Height -ge 1000000) {
        $millions = [double]$Height / 1000000.0
        return [string]::Format($script:Inv, '{0:0.#}M', $millions)
    }
    return [string]::Format($script:Inv, '{0:0.#}K', ([double]$Height / 1000.0))
}

function Get-MatchInt {
    param([string]$Line, [string]$Key)
    $m = [regex]::Match($Line, '"' + [regex]::Escape($Key) + '":(-?[0-9]+)')
    if ($m.Success) { return [long]$m.Groups[1].Value }
    return $null
}

function Get-MatchDouble {
    param([string]$Line, [string]$Key)
    $m = [regex]::Match($Line, '"' + [regex]::Escape($Key) + '":(-?[0-9]+(?:\.[0-9]+)?)')
    if ($m.Success) { return [double]$m.Groups[1].Value }
    return $null
}

# --------------------------------------------------------------------------
# log parsing
# --------------------------------------------------------------------------

function New-Sample {
    param([long]$Height, [datetime]$Time, [int]$SessionIndex)
    return [pscustomobject]@{
        Height           = $Height
        Time             = $Time
        SessionIndex     = $SessionIndex
        ElapsedSeconds   = $null
        CumRate          = $null
        WindowRate       = $null
        LogCumRate       = $null
        LogWindowRate    = $null
        L0Files          = $null
        LevelFiles       = $null
        EmptyBlockBatches        = $null
        BatchedEmptyBlocks       = $null
        EmptyBlocks              = $null
        NonEmptyBlocks           = $null
        Transactions             = $null
        Imported                 = $null
        Skipped                  = $null
    }
}

# Parse one replay log into samples, session boundaries, summary and flush telemetry.
function Get-ReplayData {
    param([string]$Path, [string]$Label)

    $data = [pscustomobject]@{
        Label          = $Label
        Path           = $Path
        Exists         = $false
        Samples        = (New-Object System.Collections.Generic.List[object])
        SessionStarts  = (New-Object System.Collections.Generic.List[datetime])
        Summaries      = (New-Object System.Collections.Generic.List[object])
        FirstLineTime  = $null
        LastLineTime   = $null
        FlushCount     = 0
        FlushRecords   = [long]0
        FlushBytes     = [long]0
        LineCount      = 0
    }

    $text = Read-SharedText $Path
    if ($null -eq $text) {
        Write-Log "log not found: $Path" 'WARN'
        return $data
    }
    $data.Exists = $true

    $starts = New-Object System.Collections.Generic.List[object]

    foreach ($line in ($text -split "`n")) {
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        $data.LineCount++

        $tsMatch = [regex]::Match($line, '"timestamp":"([^"]+)"')
        if (-not $tsMatch.Success) { continue }
        $stamp = ConvertTo-UtcTime $tsMatch.Groups[1].Value
        if ($null -eq $data.FirstLineTime) { $data.FirstLineTime = $stamp }
        $data.LastLineTime = $stamp

        if ($line.Contains('"message":"starting .acc import"')) {
            $local = Get-MatchInt $line 'local_height'
            $starts.Add([pscustomobject]@{ Time = $stamp; LocalHeight = $local })
        }

        if ($line.Contains('"message":"acc import summary"')) {
            $summary = [pscustomobject]@{
                Time          = $stamp
                Imported      = (Get-MatchInt $line 'imported')
                Skipped       = (Get-MatchInt $line 'skipped')
                FinalHeight   = (Get-MatchInt $line 'final_height')
                ElapsedSecs   = (Get-MatchDouble $line 'elapsed_secs')
                EmptyBlockBatches  = (Get-MatchInt $line 'empty_block_batches')
                BatchedEmptyBlocks = (Get-MatchInt $line 'batched_empty_blocks')
                EmptyBlocks        = (Get-MatchInt $line 'empty_blocks')
                NonEmptyBlocks     = (Get-MatchInt $line 'non_empty_blocks')
                Transactions       = (Get-MatchInt $line 'transactions')
            }
            $data.Summaries.Add($summary)
        }

        if ($line.Contains('"message":"acc import storage checkpoint flush completed"')) {
            $data.FlushCount++
            $records = Get-MatchInt $line 'application_log_records'
            $bytes   = Get-MatchInt $line 'application_log_bytes'
            if ($records) { $data.FlushRecords += $records }
            if ($bytes)   { $data.FlushBytes   += $bytes }
        }

        if (-not $line.Contains('"message":"acc import progress"')) { continue }

        $height = Get-MatchInt $line 'current_height'
        if ($null -eq $height) { continue }

        $sessionIndex = 0
        for ($s = 0; $s -lt $starts.Count; $s++) {
            if ($starts[$s].Time -le $stamp) { $sessionIndex = $s }
        }

        $sample = New-Sample -Height $height -Time $stamp -SessionIndex $sessionIndex
        $sample.LogCumRate     = Get-MatchDouble $line 'rate_blocks_per_sec'
        $sample.LogWindowRate  = Get-MatchDouble $line 'window_rate_blocks_per_sec'
        $sample.L0Files        = Get-MatchInt $line 'l0_files'
        $lvl = [regex]::Match($line, '"level_files":"([^"]*)"')
        if ($lvl.Success) { $sample.LevelFiles = $lvl.Groups[1].Value }
        $sample.EmptyBlockBatches  = Get-MatchInt $line 'empty_block_batches'
        $sample.BatchedEmptyBlocks = Get-MatchInt $line 'batched_empty_blocks'
        $sample.EmptyBlocks        = Get-MatchInt $line 'empty_blocks'
        $sample.NonEmptyBlocks     = Get-MatchInt $line 'non_empty_blocks'
        $sample.Transactions       = Get-MatchInt $line 'transactions'
        $sample.Imported           = Get-MatchInt $line 'imported'
        $sample.Skipped            = Get-MatchInt $line 'skipped'
        $data.Samples.Add($sample)
    }

    foreach ($s in $starts) { $data.SessionStarts.Add($s.Time) }

    Add-DerivedMetrics -Data $data
    return $data
}

# Cumulative (wall clock, first progress line -> sample) and trailing-window rates.
function Add-DerivedMetrics {
    param($Data)

    $samples = $Data.Samples
    if ($samples.Count -eq 0) { return }

    $firstTime = $samples[0].Time
    $base = 0
    for ($i = 0; $i -lt $samples.Count; $i++) {
        $s = $samples[$i]
        $s.ElapsedSeconds = ($s.Time - $firstTime).TotalSeconds
        if ($s.ElapsedSeconds -gt 0) {
            $s.CumRate = [double]$s.Height / $s.ElapsedSeconds
        }
        while ($base -lt $i -and $samples[$base].Height -le ($s.Height - $WindowBlocks)) { $base++ }
        if ($base -lt $i -and $samples[$base].SessionIndex -eq $s.SessionIndex) {
            $deltaSeconds = ($s.Time - $samples[$base].Time).TotalSeconds
            $deltaBlocks  = $s.Height - $samples[$base].Height
            if ($deltaSeconds -gt 0 -and $deltaBlocks -gt 0) {
                $s.WindowRate = [double]$deltaBlocks / $deltaSeconds
            }
        }
    }
}

function Find-SampleAtHeight {
    param($Samples, [long]$Height, [long]$Tolerance = 25000)

    if ($Samples.Count -eq 0) { return $null }
    $lo = 0
    $hi = $Samples.Count - 1
    while ($lo -lt $hi) {
        $mid = [int][math]::Floor(($lo + $hi) / 2)
        if ($Samples[$mid].Height -lt $Height) { $lo = $mid + 1 } else { $hi = $mid }
    }
    $best = $null
    $bestDiff = [long]::MaxValue
    for ($k = $lo - 1; $k -le $lo + 1; $k++) {
        if ($k -lt 0 -or $k -ge $Samples.Count) { continue }
        $diff = [math]::Abs($Samples[$k].Height - $Height)
        if ($diff -lt $bestDiff) { $bestDiff = $diff; $best = $Samples[$k] }
    }
    if ($null -eq $best) { return $null }
    if ($bestDiff -gt $Tolerance) { return $null }
    return $best
}

# Wall-clock span, active import time (per session) and inter-session pause.
function Get-RunTiming {
    param($Data)

    $timing = [pscustomobject]@{
        WallSeconds   = $null
        ActiveSeconds = $null
        GapSeconds    = $null
        Sessions      = 0
        FirstTime     = $null
        LastTime      = $null
        StartTime     = $null
    }
    if ($Data.Samples.Count -eq 0) { return $timing }

    $first = $Data.Samples[0].Time
    $last = $Data.Samples[$Data.Samples.Count - 1].Time
    if ($Data.Summaries.Count -gt 0) { $last = $Data.Summaries[$Data.Summaries.Count - 1].Time }

    $active = 0.0
    $sessionCount = 0
    $maxSession = 0
    foreach ($s in $Data.Samples) { if ($s.SessionIndex -gt $maxSession) { $maxSession = $s.SessionIndex } }
    for ($idx = 0; $idx -le $maxSession; $idx++) {
        $inSession = @($Data.Samples | Where-Object { $_.SessionIndex -eq $idx })
        if ($inSession.Count -eq 0) { continue }
        $sessionCount++
        $startTime = $inSession[0].Time
        if ($Data.SessionStarts.Count -gt $idx) { $startTime = $Data.SessionStarts[$idx] }
        $endTime = $inSession[$inSession.Count - 1].Time
        foreach ($summary in $Data.Summaries) {
            if ($summary.Time -ge $startTime -and $summary.Time -le $endTime.AddMinutes(5)) { $endTime = $summary.Time }
        }
        $span = ($endTime - $startTime).TotalSeconds
        if ($span -gt 0) { $active += $span }
    }

    $timing.Sessions      = $sessionCount
    $timing.FirstTime     = $first
    $timing.LastTime      = $last
    if ($Data.SessionStarts.Count -gt 0) { $timing.StartTime = $Data.SessionStarts[0] } else { $timing.StartTime = $first }
    $timing.WallSeconds   = ($last - $first).TotalSeconds
    $timing.ActiveSeconds = $active
    $timing.GapSeconds    = $timing.WallSeconds - $active
    if ($timing.GapSeconds -lt 0) { $timing.GapSeconds = 0 }
    return $timing
}

# --------------------------------------------------------------------------
# step 1: wait for the replay process
# --------------------------------------------------------------------------

function Get-LastLogLine {
    param([string]$Path)
    $text = Read-SharedText $Path
    if ($null -eq $text) { return $null }
    $lines = $text -split "`n"
    for ($i = $lines.Count - 1; $i -ge 0; $i--) {
        if (-not [string]::IsNullOrWhiteSpace($lines[$i])) { return $lines[$i].Trim() }
    }
    return $null
}

function Get-ReplayProgressSnapshot {
    param([string]$Line)
    if ($null -eq $Line) { return 'no log output yet' }
    if (-not $Line.Contains('"message":"acc import progress"')) {
        if ($Line.Length -gt 220) { return $Line.Substring(0, 220) + '...' }
        return $Line
    }
    $height = Get-MatchInt $Line 'current_height'
    $cum    = Get-MatchDouble $Line 'rate_blocks_per_sec'
    $win    = Get-MatchDouble $Line 'window_rate_blocks_per_sec'
    $eta    = 'n/a'
    if ($height -and $cum -and $cum -gt 0) {
        $remaining = $ExpectedFinalHeight - $height
        if ($remaining -gt 0) { $eta = Format-Duration ($remaining / $cum) }
    }
    $text = 'height=' + (Format-Number $height) + ' cum=' + (Format-Number $cum) + ' blk/s'
    if ($win) { $text += ' win=' + (Format-Number $win) + ' blk/s' }
    $text += ' remaining=' + $eta
    return $text
}

function Wait-ReplayExit {
    param([int]$ProcessId)

    if ($SkipWait) {
        Write-Log 'wait step skipped (-SkipWait)'
        return $true
    }

    $deadline = (Get-Date).AddHours($MaxWaitHours)
    $waited = 0
    $lastSize = -1
    $lastGrowth = Get-Date

    while ($true) {
        $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
        if (-not $proc) {
            if ($waited -gt 0) {
                Write-Log ('replay process ' + $ProcessId + ' exited after ' + (Format-Duration $waited) + ' of monitoring')
            } else {
                Write-Log ('replay process ' + $ProcessId + ' is not running; skipping wait (idempotent path)')
            }
            return $true
        }
        if ($proc.ProcessName -ne 'neo-node') {
            Write-Log ('PID ' + $ProcessId + ' now belongs to "' + $proc.ProcessName + '", not neo-node; treating the replay as finished') 'WARN'
            return $true
        }

        $snapshot = Get-ReplayProgressSnapshot (Get-LastLogLine $ReplayLog)
        $size = -1
        $item = Get-Item -LiteralPath $ReplayLog -ErrorAction SilentlyContinue
        if ($item) { $size = $item.Length }
        if ($size -ne $lastSize) { $lastSize = $size; $lastGrowth = Get-Date }
        $stalled = ((Get-Date) - $lastGrowth).TotalMinutes
        $stallNote = ''
        if ($stalled -ge 15) {
            $stallNote = '  [WARN no log growth for ' + (Format-Number $stalled 0) + ' min]'
        }

        Write-Log ('running: ' + $snapshot + ' | pid cpu=' + (Format-Number $proc.CPU 0) + 's ws=' +
                   (Format-Number ($proc.WorkingSet64 / 1MB) 0) + 'MiB' + $stallNote)
        $waited += $PollSeconds

        if ((Get-Date) -gt $deadline) {
            Write-Log ('wait deadline of ' + $MaxWaitHours + ' h reached; the replay is still running') 'ERROR'
            return $false
        }
        Start-Sleep -Seconds $PollSeconds
    }
}

# --------------------------------------------------------------------------
# step 2: validate the terminal summary
# --------------------------------------------------------------------------

function Test-ReplayCompletion {
    param($Data)

    $result = [pscustomobject]@{
        Ok        = $false
        Reason    = ''
        Summary   = $null
        FinalHeight = $null
        Imported  = $null
        Skipped   = $null
        ElapsedSecs = $null
    }

    if (-not $Data.Exists) {
        $result.Reason = 'replay log does not exist: ' + $Data.Path
        return $result
    }
    if ($Data.Summaries.Count -eq 0) {
        $lastSample = $null
        if ($Data.Samples.Count -gt 0) { $lastSample = $Data.Samples[$Data.Samples.Count - 1] }
        $where = 'no progress line seen'
        if ($lastSample) { $where = 'last progress height ' + (Format-Number $lastSample.Height) }
        $result.Reason = '"acc import summary" is missing from the log (' + $where + ')'
        return $result
    }

    $summary = $Data.Summaries[$Data.Summaries.Count - 1]
    $result.Summary     = $summary
    $result.FinalHeight = $summary.FinalHeight
    $result.Imported    = $summary.Imported
    $result.Skipped     = $summary.Skipped
    $result.ElapsedSecs = $summary.ElapsedSecs

    if ($null -eq $summary.FinalHeight -or $summary.FinalHeight -ne $ExpectedFinalHeight) {
        $result.Reason = 'final_height is ' + (Format-Number $summary.FinalHeight) +
                         ', expected ' + (Format-Number $ExpectedFinalHeight)
        return $result
    }
    if ($null -eq $summary.Imported -or $summary.Imported -le 0) {
        $result.Reason = 'imported is ' + (Format-Number $summary.Imported) + ', expected > 0'
        return $result
    }

    $result.Ok = $true
    return $result
}

# --------------------------------------------------------------------------
# step 3: state-root sweep
# --------------------------------------------------------------------------

function Invoke-StateRootSweep {
    param([string]$Binary)

    $outcome = [pscustomobject]@{
        Ran        = $false
        Binary     = $Binary
        ExitCode   = $null
        Verdict    = 'not run'
        Detail     = ''
        Result     = $null
        Tail       = @()
    }

    $sweepArgs = @(
        '--db', $DbPath,
        '--reference', $Reference,
        '--start-height', '0',
        '--end-height', [string]$ExpectedFinalHeight,
        '--output', $SweepOutput,
        '--temp-dir', $TempDir,
        '--mode', 'read-only',
        '--progress-every', '500000'
    )

    Write-Log ('running sweep: "' + $Binary + '" ' + ($sweepArgs -join ' '))
    $started = Get-Date
    $captured = & $Binary @sweepArgs 2>&1 | ForEach-Object { $_.ToString() }
    $exitCode = $LASTEXITCODE
    $elapsed = ((Get-Date) - $started).TotalSeconds

    $captured | Set-Content -LiteralPath $SweepLog -Encoding UTF8
    $tail = @($captured | Select-Object -Last 12)

    $outcome.Ran      = $true
    $outcome.ExitCode = $exitCode
    $outcome.Tail     = $tail

    Write-Log ('sweep exit code ' + $exitCode + ' after ' + (Format-Duration $elapsed))

    if (Test-Path -LiteralPath $SweepOutput) {
        try {
            $json = Get-Content -LiteralPath $SweepOutput -Raw | ConvertFrom-Json
            $outcome.Result = $json
            $outcome.Verdict = [string]$json.verdict
            if ($json.comparison) {
                $outcome.Detail = 'compared=' + (Format-Number $json.comparison.compared_heights) +
                                  ' matches=' + (Format-Number $json.comparison.matches) +
                                  ' mismatches=' + (Format-Number $json.comparison.mismatches) +
                                  ' missing_local=' + (Format-Number $json.comparison.missing_local) +
                                  ' missing_reference=' + (Format-Number $json.comparison.missing_reference) +
                                  ' decode_errors=' + (Format-Number $json.comparison.decode_errors)
            }
        } catch {
            $outcome.Verdict = 'unreadable result'
            $outcome.Detail  = $_.Exception.Message
        }
    } else {
        $outcome.Verdict = 'no result file'
        $outcome.Detail  = 'the sweep did not write ' + $SweepOutput
    }

    switch ($exitCode) {
        0 { Write-Log ('sweep verdict: PASS - ' + $outcome.Detail) }
        1 { Write-Log ('sweep verdict: FAIL (verification) - ' + $outcome.Detail) 'ERROR' }
        2 { Write-Log ('sweep verdict: FAIL (usage error) - ' + $outcome.Detail) 'ERROR' }
        3 { Write-Log ('sweep verdict: FAIL (operational error) - ' + $outcome.Detail) 'ERROR' }
        default { Write-Log ('sweep verdict: FAIL (unexpected exit code) - ' + $outcome.Detail) 'ERROR' }
    }
    return $outcome
}

# --------------------------------------------------------------------------
# step 4: report
# --------------------------------------------------------------------------

$reportHeights = @(
    500000, 1000000, 1500000, 2000000, 3000000, 4000000, 5000000,
    6000000, 7000000, 7500000, 8000000, 8500000, 9000000, 9500000,
    10000000, 10500000, 11000000, 11500000, 12000000, 12500000, 13000000
)

function Get-RateStats {
    param($Data)
    $windowRates = @($Data.Samples | Where-Object { $null -ne $_.WindowRate -and $_.Height -ge 100000 } | ForEach-Object { $_.WindowRate })
    $logRates    = @($Data.Samples | Where-Object { $null -ne $_.LogWindowRate -and $_.Height -ge 100000 } | ForEach-Object { $_.LogWindowRate })
    $stats = [pscustomobject]@{
        PeakWindow    = $null
        SlowestWindow = $null
        MeanWindow    = $null
        PeakLogWindow = $null
        SlowestLogWindow = $null
        MeanLogWindow = $null
        WindowSamples = $windowRates.Count
    }
    if ($windowRates.Count -gt 0) {
        $stats.PeakWindow    = ($windowRates | Measure-Object -Maximum).Maximum
        $stats.SlowestWindow = ($windowRates | Measure-Object -Minimum).Minimum
        $stats.MeanWindow    = ($windowRates | Measure-Object -Average).Average
    }
    if ($logRates.Count -gt 0) {
        $stats.PeakLogWindow    = ($logRates | Measure-Object -Maximum).Maximum
        $stats.SlowestLogWindow = ($logRates | Measure-Object -Minimum).Minimum
        $stats.MeanLogWindow    = ($logRates | Measure-Object -Average).Average
    }
    return $stats
}

function Build-ComparisonReport {
    param(
        $ControlData, $ControlTiming, $ControlStats,
        $OptimizedData, $OptimizedTiming, $OptimizedStats,
        $Completion, $Sweep, [bool]$Partial
    )

    $sb = New-Object System.Text.StringBuilder
    $append = { param([string]$Text = '') [void]$sb.AppendLine($Text) }
    $now = Get-Date

    & $append '# A/B Replay Comparison - Neo N3 MainNet 0 -> 13,141,250'
    & $append
    & $append ('Generated: ' + $now.ToString('yyyy-MM-dd HH:mm:ss') + ' local (' + $now.ToUniversalTime().ToString('yyyy-MM-dd HH:mm:ss') + ' UTC)')
    & $append
    if ($Partial) {
        & $append '> **PARTIAL / IN PROGRESS** - the optimized replay has not reported a terminal summary yet,'
        & $append '> so heights the run did not reach are shown as `pending`.'
        & $append
    }
    & $append ('| Input | Path |')
    & $append ('| --- | --- |')
    & $append ('| Control replay log (A) | `' + $ControlData.Path + '` |')
    & $append ('| Optimized replay log (B) | `' + $OptimizedData.Path + '` |')
    & $append ('| Optimized database (sweep target, read-only) | `' + $DbPath + '` |')
    & $append ('| State-root reference | `' + $Reference + '` |')
    & $append ('| Sweep result | `' + $SweepOutput + '` |')
    & $append

    # ---- headline -------------------------------------------------------
    & $append '## 1. Headline'
    & $append
    & $append ('| Metric | Control (A) | Optimized (B) | Delta (B vs A) |')
    & $append ('| --- | ---: | ---: | ---: |')

    $controlWall = $ControlTiming.WallSeconds
    $optWall     = $OptimizedTiming.WallSeconds
    $controlActive = $ControlTiming.ActiveSeconds
    $optActive     = $OptimizedTiming.ActiveSeconds

    & $append ('| Wall clock, first progress -> terminal state | ' + (Format-Duration $controlWall) + ' | ' + (Format-Duration $optWall) + ' | ' + (Format-Delta $controlWall $optWall) + ' |')
    & $append ('| Active import time (session spans) | ' + (Format-Duration $controlActive) + ' | ' + (Format-Duration $optActive) + ' | ' + (Format-Delta $controlActive $optActive) + ' |')
    & $append ('| Sessions (restarts) | ' + $ControlTiming.Sessions + ' | ' + $OptimizedTiming.Sessions + ' | |')
    & $append ('| Wall minus active (startup/pause) | ' + (Format-Duration $ControlTiming.GapSeconds) + ' | ' + (Format-Duration $OptimizedTiming.GapSeconds) + ' | |')

    $controlOverall = $null
    $optOverall = $null
    $controlHeight = $null
    if ($ControlData.Samples.Count -gt 0) { $controlHeight = $ControlData.Samples[$ControlData.Samples.Count - 1].Height }
    if ($controlActive -and $controlActive -gt 0 -and $controlHeight) {
        $controlOverall = [double]$controlHeight / $controlActive
    }
    $optHeight = $null
    if ($OptimizedData.Samples.Count -gt 0) { $optHeight = $OptimizedData.Samples[$OptimizedData.Samples.Count - 1].Height }
    if ($optActive -and $optActive -gt 0 -and $optHeight) {
        $optOverall = [double]$optHeight / $optActive
    }
    & $append ('| Overall throughput (blocks/active-second) | ' + (Format-Number $controlOverall) + ' | ' + (Format-Number $optOverall) + ' | ' + (Format-Delta $controlOverall $optOverall) + ' |')
    & $append ('| Heights covered (A: ' + (Format-Number $controlHeight) + ' / B: ' + (Format-Number $optHeight) + ' of ' + (Format-Number $ExpectedFinalHeight) + ') | | | |')
    & $append
    if ($controlWall -and $optWall -and $controlWall -gt 0) {
        & $append ('Overall improvement on wall clock: **' + (Format-Number ((($controlWall - $optWall) / $controlWall) * 100) 1) + '%** (' +
                   (Format-Duration $controlWall) + ' -> ' + (Format-Duration $optWall) + ').')
            & $append
    }
    if ($controlActive -and $optActive -and $controlActive -gt 0) {
        & $append ('Overall improvement on active import time: **' + (Format-Number ((($controlActive - $optActive) / $controlActive) * 100) 1) + '%** (' +
                   (Format-Duration $controlActive) + ' -> ' + (Format-Duration $optActive) + ').')
        & $append
    }

    # ---- matched height table -------------------------------------------
    & $append '## 2. Matched-height throughput'
    & $append
    & $append 'Cumulative rates are wall clock from the first progress line of the run to the sample nearest the'
    & $append ('requested height (tolerance +/- 25,000 blocks). The optimized window rate is a trailing 100,000-block')
    & $append 'window ending at that sample (session gaps excluded), i.e. the instantaneous steady-state rate.'
    & $append
    & $append '| Height | Control cum (blk/s) | Optimized cum (blk/s) | Optimized window (blk/s) | Delta (cum) |'
    & $append '| ---: | ---: | ---: | ---: | ---: |'

    foreach ($height in $reportHeights) {
        $ctrl = Find-SampleAtHeight -Samples $ControlData.Samples -Height $height
        $opt  = Find-SampleAtHeight -Samples $OptimizedData.Samples -Height $height

        $ctrlCum = 'n/a'
        if ($ctrl -and $null -ne $ctrl.CumRate) { $ctrlCum = Format-Number $ctrl.CumRate }
        $optCum = 'n/a'
        if ($opt -and $null -ne $opt.CumRate) { $optCum = Format-Number $opt.CumRate }
        elseif ($Partial) { $optCum = 'pending' }
        $optWin = 'n/a'
        if ($opt -and $null -ne $opt.WindowRate) { $optWin = Format-Number $opt.WindowRate }
        elseif ($opt) { $optWin = 'n/a (session edge)' }

        $delta = 'n/a'
        if ($ctrl -and $opt -and $null -ne $ctrl.CumRate -and $null -ne $opt.CumRate -and $ctrl.CumRate -gt 0) {
            $delta = (Format-Number ((($opt.CumRate - $ctrl.CumRate) / $ctrl.CumRate) * 100) 1) + '%'
        }
        & $append ('| ' + (Format-HeightLabel $height) + ' | ' + $ctrlCum + ' | ' + $optCum + ' | ' + $optWin + ' | ' + $delta + ' |')
    }
    & $append

    # ---- window extremes ------------------------------------------------
    & $append '## 3. Window-rate extremes (trailing 100,000 blocks, heights >= 100K)'
    & $append
    & $append '| Metric | Control (A) | Optimized (B) |'
    & $append '| --- | ---: | ---: |'
    & $append ('| Peak window rate (blk/s) | ' + (Format-Number $ControlStats.PeakWindow) + ' | ' + (Format-Number $OptimizedStats.PeakWindow) + ' |')
    & $append ('| Slowest window rate (blk/s) | ' + (Format-Number $ControlStats.SlowestWindow) + ' | ' + (Format-Number $OptimizedStats.SlowestWindow) + ' |')
    & $append ('| Mean window rate (blk/s) | ' + (Format-Number $ControlStats.MeanWindow) + ' | ' + (Format-Number $OptimizedStats.MeanWindow) + ' |')
    & $append ('| Log-reported 1K-window peak / slowest / mean (blk/s) | ' +
               (Format-Number $ControlStats.PeakLogWindow) + ' / ' + (Format-Number $ControlStats.SlowestLogWindow) + ' / ' + (Format-Number $ControlStats.MeanLogWindow) + ' | ' +
               (Format-Number $OptimizedStats.PeakLogWindow) + ' / ' + (Format-Number $OptimizedStats.SlowestLogWindow) + ' / ' + (Format-Number $OptimizedStats.MeanLogWindow) + ' |')
    & $append

    # ---- optimized telemetry -------------------------------------------
    $lastSample = $null
    if ($OptimizedData.Samples.Count -gt 0) { $lastSample = $OptimizedData.Samples[$OptimizedData.Samples.Count - 1] }
    $batches = $null
    $batched = $null
    $emptyBlocks = $null
    $nonEmpty    = $null
    $txs         = $null
    if ($Completion.Summary) {
        $batches = $Completion.Summary.EmptyBlockBatches
        $batched = $Completion.Summary.BatchedEmptyBlocks
        $emptyBlocks = $Completion.Summary.EmptyBlocks
        $nonEmpty = $Completion.Summary.NonEmptyBlocks
        $txs = $Completion.Summary.Transactions
    }
    if ($null -eq $batches -and $lastSample) { $batches = $lastSample.EmptyBlockBatches }
    if ($null -eq $batched -and $lastSample) { $batched = $lastSample.BatchedEmptyBlocks }
    if ($null -eq $emptyBlocks -and $lastSample) { $emptyBlocks = $lastSample.EmptyBlocks }
    if ($null -eq $nonEmpty -and $lastSample) { $nonEmpty = $lastSample.NonEmptyBlocks }
    if ($null -eq $txs -and $lastSample) { $txs = $lastSample.Transactions }

    & $append '## 4. Optimized-run telemetry'
    & $append
    & $append '| Metric | Value |'
    & $append '| --- | ---: |'
    & $append ('| Imported blocks (summary) | ' + (Format-Number $Completion.Imported) + ' |')
    & $append ('| Skipped blocks (summary) | ' + (Format-Number $Completion.Skipped) + ' |')
    & $append ('| Final height (summary) | ' + (Format-Number $Completion.FinalHeight) + ' |')
    & $append ('| Import elapsed (summary, s) | ' + (Format-Number $Completion.ElapsedSecs 1) + ' |')
    & $append ('| Empty blocks | ' + (Format-Number $emptyBlocks) + ' |')
    & $append ('| Non-empty blocks | ' + (Format-Number $nonEmpty) + ' |')
    & $append ('| Transactions | ' + (Format-Number $txs) + ' |')
    & $append ('| empty_block_batches (write batches) | ' + (Format-Number $batches) + ' |')
    & $append ('| batched_empty_blocks (blocks coalesced into those batches) | ' + (Format-Number $batched) + ' |')
    $perBatch = $null
    if ($batches -and $batches -gt 0 -and $batched) { $perBatch = [double]$batched / [double]$batches }
    & $append ('| Mean empty blocks per batch | ' + (Format-Number $perBatch 2) + ' |')
    $batchReduction = $null
    if ($emptyBlocks -and $batches -and $emptyBlocks -gt 0) { $batchReduction = (1.0 - ([double]$batches / [double]$emptyBlocks)) * 100.0 }
    & $append ('| Empty-block write-batch reduction vs one-write-per-block | ' + (Format-Number $batchReduction 1) + '% |')
    & $append ('| Application-log flush events (checkpoint flushes) | ' + (Format-Number $OptimizedData.FlushCount) + ' |')
    & $append ('| Application-log records flushed | ' + (Format-Number $OptimizedData.FlushRecords) + ' |')
    & $append ('| Application-log bytes flushed | ' + (Format-Number $OptimizedData.FlushBytes) + ' |')
    if ($lastSample) {
        & $append ('| Final L0 file count | ' + (Format-Number $lastSample.L0Files) + ' |')
        if ($lastSample.LevelFiles) {
            & $append ('| Final level layout | `' + $lastSample.LevelFiles + '` |')
        }
    }
    & $append

    # ---- sweep ----------------------------------------------------------
    & $append '## 5. State-root sweep verdict'
    & $append
    if (-not $Sweep.Ran) {
        & $append ('**Sweep not executed.** ' + $Sweep.Detail)
        & $append
    } else {
        & $append ('| Metric | Value |')
        & $append ('| --- | --- |')
        & $append ('| Binary | `' + $Sweep.Binary + '` |')
        & $append ('| Exit code | ' + $Sweep.ExitCode + ' |')
        & $append ('| Verdict | **' + $Sweep.Verdict + '** |')
        & $append ('| Detail | ' + $Sweep.Detail + ' |')
        if ($Sweep.Result -and $Sweep.Result.timing) {
            & $append ('| Compared heights / elapsed | ' + (Format-Number $Sweep.Result.comparison.compared_heights) + ' in ' + (Format-Number $Sweep.Result.timing.elapsed_seconds 1) + ' s |')
        }
        if ($Sweep.Result -and $Sweep.Result.comparison -and $Sweep.Result.comparison.first_mismatch) {
            $mm = $Sweep.Result.comparison.first_mismatch
            & $append ('| First mismatch | height ' + $mm.height + ' local=' + $mm.local + ' reference=' + $mm.reference + ' |')
        }
        & $append
        if ($Sweep.Tail.Count -gt 0) {
            & $append 'Sweep stdout tail:'
            & $append
            & $append '```'
            foreach ($line in $Sweep.Tail) { & $append $line }
            & $append '```'
            & $append
        }
    }

    # ---- caveats --------------------------------------------------------
    & $append '## 6. Caveats'
    & $append
    & $append '- The control run (A) was produced by a **different binary**: no empty-block batch coalescing,'
    & $append '  no write coalescing, and a 256 MiB RocksDB block cache. It is the pre-optimization baseline.'
    & $append ('- The control log contains **' + $ControlTiming.Sessions + ' session(s)** (it was restarted), and its rate curve')
    & $append '  includes the L0 compaction-backlog period (6.1M-13.1M). Its cumulative rates therefore'
    & $append '  include an operational stall that is unrelated to the code under test.'
    & $append '- The optimized run (B) is a single fresh 0-13,141,250 replay with batching, coalescing and the'
    & $append '  larger cache enabled from height 0, so the A/B delta mixes code effects with run geometry.'
    & $append '- Cumulative rates are wall clock; window rates are trailing 100,000-block averages and cannot'
    & $append '  cross a session boundary (shown as `n/a (session edge)`).'
    & $append '- The sweep reads the optimized database with `DB::open_for_read_only`; it never writes to it.'
    & $append

    return $sb.ToString()
}

function Format-Delta {
    param($Baseline, $Candidate)
    if ($null -eq $Baseline -or $null -eq $Candidate -or $Baseline -eq 0) { return 'n/a' }
    $pct = (([double]$Candidate - [double]$Baseline) / [double]$Baseline) * 100.0
    $sign = '+'
    if ($pct -lt 0) { $sign = '' }
    return $sign + (Format-Number $pct 1) + '%'
}

function Save-Report {
    param([string]$Text, [string]$Path)
    $parent = Split-Path -Parent $Path
    if ($parent -and -not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }
    [System.IO.File]::WriteAllText($Path, $Text, $script:Utf8NoBom)
    Write-Log ('report written to ' + $Path + ' (' + (Format-Number ($Text.Length / 1KB) 1) + ' KiB)')
}

# --------------------------------------------------------------------------
# main
# --------------------------------------------------------------------------

$exitCode = 0

try {
    Write-Log '=== Task 57 post-replay verification start ==='
    Write-Log ('replay pid=' + $ReplayPid + ' log=' + $ReplayLog)

    foreach ($dir in @((Split-Path -Parent $ReportPath), (Split-Path -Parent $SweepOutput), $TempDir, (Split-Path -Parent $SweepLog))) {
        if ($dir -and -not (Test-Path -LiteralPath $dir)) {
            New-Item -ItemType Directory -Path $dir -Force | Out-Null
            Write-Log ('created directory ' + $dir)
        }
    }

    # 1. wait
    $exited = Wait-ReplayExit -ProcessId $ReplayPid
    if (-not $exited) {
        Write-Log 'the replay did not finish inside the wait window; aborting without running the sweep' 'ERROR'
        exit 4
    }
    Start-Sleep -Seconds 3   # let the final log lines reach the file

    # 2. validate
    $optimized = Get-ReplayData -Path $ReplayLog -Label 'optimized'
    $completion = Test-ReplayCompletion -Data $optimized

    if (-not $completion.Ok -and -not $AllowIncomplete) {
        Write-Log ('replay did not complete cleanly: ' + $completion.Reason) 'ERROR'
        $failure = @()
        $failure += '# A/B Replay Comparison - Neo N3 MainNet 0 -> 13,141,250'
        $failure += ''
        $failure += ('Generated: ' + (Get-Date).ToString('yyyy-MM-dd HH:mm:ss') + ' local')
        $failure += ''
        $failure += '## Result: REPLAY DID NOT COMPLETE'
        $failure += ''
        $failure += ('- ' + $completion.Reason)
        $failure += ('- Log: `' + $ReplayLog + '`')
        $failure += ('- Expected final_height: ' + (Format-Number $ExpectedFinalHeight))
        $failure += '- The state-root sweep and the A/B comparison were not produced because the optimized'
        $failure += '  replay has no valid terminal state.'
        $text = ($failure -join [Environment]::NewLine) + [Environment]::NewLine
        Save-Report -Text $text -Path $ReportPath
        Write-Host $text
        Write-Log 'post-replay verification failed at the replay-completion gate' 'ERROR'
        exit 1
    }
    if (-not $completion.Ok) {
        Write-Log ('replay completion is not final (' + $completion.Reason + '); continuing because -AllowIncomplete was set') 'WARN'
    } else {
        Write-Log ('replay summary ok: final_height=' + (Format-Number $completion.FinalHeight) +
                   ' imported=' + (Format-Number $completion.Imported) +
                   ' skipped=' + (Format-Number $completion.Skipped) +
                   ' elapsed=' + (Format-Number $completion.ElapsedSecs 1) + 's')
    }

    # 3. sweep
    $sweep = [pscustomobject]@{ Ran = $false; Binary = $null; ExitCode = $null; Verdict = 'not run'; Detail = ''; Result = $null; Tail = @() }
    if ($SkipSweep) {
        Write-Log 'sweep skipped (-SkipSweep)'
        if (Test-Path -LiteralPath $SweepOutput) {
            try {
                $json = Get-Content -LiteralPath $SweepOutput -Raw | ConvertFrom-Json
                $sweep.Ran = $true
                $sweep.Binary = '(reused result)'
                $sweep.ExitCode = $json.exit_code
                $sweep.Verdict = [string]$json.verdict
                if ($json.comparison) {
                    $sweep.Detail = 'compared=' + (Format-Number $json.comparison.compared_heights) +
                                    ' matches=' + (Format-Number $json.comparison.matches) +
                                    ' mismatches=' + (Format-Number $json.comparison.mismatches)
                }
                $sweep.Result = $json
                Write-Log ('reused sweep result: verdict=' + $sweep.Verdict + ' ' + $sweep.Detail)
            } catch {
                $sweep.Detail = 'existing result.json is unreadable: ' + $_.Exception.Message
                Write-Log $sweep.Detail 'WARN'
            }
        } else {
            $sweep.Detail = 'no existing result at ' + $SweepOutput
            Write-Log $sweep.Detail 'WARN'
        }
    } else {
        $binary = $null
        if (Test-Path -LiteralPath $ReleaseSweep) {
            $binary = $ReleaseSweep
        } elseif (Test-Path -LiteralPath $DebugSweep) {
            $binary = $DebugSweep
            Write-Log 'release sweep binary missing; falling back to the debug build' 'WARN'
        }

        if ($null -eq $binary) {
            $sweep.Detail = 'state-root-sweep.exe not found at ' + $ReleaseSweep + ' or ' + $DebugSweep +
                            ' - rebuild with: cargo build -p state-root-diff --release --bin state-root-sweep'
            Write-Log $sweep.Detail 'ERROR'
            $exitCode = 2
        } elseif (-not (Test-Path -LiteralPath $DbPath)) {
            $sweep.Detail = 'replay database directory not found: ' + $DbPath
            Write-Log $sweep.Detail 'ERROR'
            $exitCode = 2
        } elseif (-not (Test-Path -LiteralPath $Reference)) {
            $sweep.Detail = 'reference file not found: ' + $Reference
            Write-Log $sweep.Detail 'ERROR'
            $exitCode = 2
        } else {
            $sweep = Invoke-StateRootSweep -Binary $binary
            if ($sweep.ExitCode -ne 0) { $exitCode = 2 }
        }
    }

    # 4. report
    $control = Get-ReplayData -Path $ControlLog -Label 'control'
    if ($control.Samples.Count -eq 0) {
        Write-Log ('control log produced no progress samples: ' + $ControlLog) 'ERROR'
        $exitCode = 3
    } else {
        $controlTiming = Get-RunTiming -Data $control
        $optimizedTiming = Get-RunTiming -Data $optimized
        $controlStats = Get-RateStats -Data $control
        $optimizedStats = Get-RateStats -Data $optimized

        Write-Log ('control: wall=' + (Format-Duration $controlTiming.WallSeconds) +
                   ' active=' + (Format-Duration $controlTiming.ActiveSeconds) +
                   ' sessions=' + $controlTiming.Sessions)
        Write-Log ('optimized: wall=' + (Format-Duration $optimizedTiming.WallSeconds) +
                   ' active=' + (Format-Duration $optimizedTiming.ActiveSeconds) +
                   ' sessions=' + $optimizedTiming.Sessions)

        $partial = -not $completion.Ok
        $report = Build-ComparisonReport -ControlData $control -ControlTiming $controlTiming -ControlStats $controlStats `
                                        -OptimizedData $optimized -OptimizedTiming $optimizedTiming -OptimizedStats $optimizedStats `
                                        -Completion $completion -Sweep $sweep -Partial $partial

        Save-Report -Text $report -Path $ReportPath
        Write-Log '=== report ==='
        Write-Host ''
        Write-Host $report
    }

    # final verdict line
    Write-Log ('=== Task 57 post-replay verification finished in ' + (Format-Duration $script:Stopwatch.Elapsed.TotalSeconds) +
               ' with exit code ' + $exitCode + ' ===')
    exit $exitCode
} catch {
    Write-Log ('unhandled error: ' + $_.Exception.Message + ' at ' + $_.InvocationInfo.PositionMessage) 'ERROR'
    exit 99
}
