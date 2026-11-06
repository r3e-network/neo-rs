$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Set-Location -Path $PSScriptRoot\..

# Task 184 Option B: plain-copy the neo-core (simple API) versions of
# tokens_tracker and application_logs into neo-rpc, rewriting `crate::X`
# to `neo_core::X` while PRESERVING module self-references
# (`crate::tokens_tracker` / `crate::application_logs`).

$mods = @('tokens_tracker', 'application_logs')

# regex: crate:: NOT followed by tokens_tracker/application_logs -> neo_core::
$rewrite = {
    param($text)
    [regex]::Replace($text, 'crate::(?!tokens_tracker\b|application_logs\b)', 'neo_core::')
}

foreach ($mod in $mods) {
    $files = git ls-files "neo-core/src/$mod/"
    foreach ($f in $files) {
        if ([string]::IsNullOrWhiteSpace($f)) { continue }
        $rel = $f -replace '^neo-core/', 'neo-rpc/'
        $lines = git show "HEAD:$f"
        $content = ($lines -join "`n")
        # normalize CRLF that git may emit; keep LF for source parity
        $content = $content -replace "`r`n", "`n"
        $new = & $rewrite $content
        $dest = Join-Path (Get-Location) ($rel -replace '/', '\')
        $destDir = Split-Path $dest -Parent
        if (-not (Test-Path $destDir)) { New-Item -ItemType Directory -Force -Path $destDir | Out-Null }
        # write without BOM, preserve trailing newline from git blob
        $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllText($dest, $new.TrimEnd("`r", "`n") + "`n", $utf8NoBom)
        Write-Host "WROTE $rel"
    }
}
