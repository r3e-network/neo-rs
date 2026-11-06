$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Set-Location -Path $PSScriptRoot\..

# Task 184: relocate the NEP-17 C# parity integration test from neo-core to
# neo-rpc. The tracker types now live in neo-rpc (simple/plain API), while the
# supporting types (Block, ApplicationExecuted, NeoSystem, persistence, ...) are
# still reached through the neo-core backward-compat facade.

$src = 'neo-core/tests/tokens_tracker_nep17_csharp_parity.rs'
$lines = git show "HEAD:$src"
$content = ($lines -join "`n") -replace "`r`n", "`n"

# tracker types move to neo-rpc
$content = $content -replace 'neo_core::tokens_tracker', 'neo_rpc::tokens_tracker'
# neo-rpc gates the tracker module behind the `server` feature (neo-core dep)
$content = $content -replace '#!\[cfg\(feature = "runtime"\)\]', '#![cfg(feature = "server")]'

$dest = Join-Path (Get-Location) 'neo-rpc\tests\tokens_tracker_nep17_csharp_parity.rs'
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText($dest, $content.TrimEnd("`r", "`n") + "`n", $utf8NoBom)
Write-Host "WROTE neo-rpc/tests/tokens_tracker_nep17_csharp_parity.rs"
