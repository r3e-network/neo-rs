# Task 50 phase A: verify-mode import 100,001..300,000 that KEEPS RUNNING
# afterwards so the health/metrics server is live. `--import-only` is
# intentionally absent: the verify-mode counters
# (`neo_fast_path_verification_mismatches_total`, `neo_fast_path_direct_attempts_total`,
# `neo_fast_path_canonical_fallbacks_total`, `neo_fast_path_mode`) are published
# by the metrics pump, which only starts in the serve phase, after the import.
# This process is stopped manually once /metrics has been scraped.
Set-Location 'd:\Git\neo-rs'

$env:NEO_ROCKSDB_BATCH_PROFILE = 'high_throughput'
$env:NEO_IMPORT_PROGRESS_INTERVAL = '10000'
$env:NEO_IMPORT_FLUSH_INTERVAL = '10000'
$env:NEO_IMPORT_STOP_HEIGHT = '300000'

$node = 'd:\Git\neo-rs\target\mainnet-replay-build\release\neo-node.exe'
$cfg = 'd:\Git\neo-rs\config\mainnet-verify-opt.toml'
$acc = 'd:\Git\neo-rs\data\bootstrap\chain.0.acc'

$started = Get-Date
& $node --config $cfg --import-acc $acc --health-port 10356 *> 'd:\Git\neo-rs\.t50-verify-counts.out.log'
$code = $LASTEXITCODE
$elapsed = ((Get-Date) - $started).TotalSeconds

"exit=$code" | Set-Content 'd:\Git\neo-rs\.t50-verify-counts.exit.log'
"elapsed_seconds=$([math]::Round($elapsed, 1))" | Add-Content 'd:\Git\neo-rs\.t50-verify-counts.exit.log'