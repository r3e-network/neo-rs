# Task 50 launcher: parallel verify-mode ACC replay (isolated from the canonical
# replay running as PID 48516 in data/mainnet-replay-fresh).
Set-Location 'd:\Git\neo-rs'

$env:NEO_ROCKSDB_BATCH_PROFILE = 'high_throughput'
$env:NEO_IMPORT_PROGRESS_INTERVAL = '10000'
$env:NEO_IMPORT_FLUSH_INTERVAL = '10000'
$env:NEO_IMPORT_STOP_HEIGHT = '3100000'

$node = 'd:\Git\neo-rs\target\mainnet-replay-build\release\neo-node.exe'
$cfg = 'd:\Git\neo-rs\config\mainnet-verify-opt.toml'
$acc = 'd:\Git\neo-rs\data\bootstrap\chain.0.acc'

$started = Get-Date
& $node --config $cfg --import-acc $acc --import-only *> 'd:\Git\neo-rs\.t50-verify-long.out.log'
$code = $LASTEXITCODE
$elapsed = ((Get-Date) - $started).TotalSeconds

"exit=$code" | Set-Content 'd:\Git\neo-rs\.t50-verify-long.exit.log'
"elapsed_seconds=$([math]::Round($elapsed, 1))" | Add-Content 'd:\Git\neo-rs\.t50-verify-long.exit.log'