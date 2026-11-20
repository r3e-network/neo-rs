$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$logs = Join-Path $PSScriptRoot 'logs'
$process = Start-Process -FilePath (Join-Path $root '.target-t30/debug/neo-validator.exe') -ArgumentList @('--config', (Join-Path $PSScriptRoot 'neo-validator.toml')) -WorkingDirectory $root -RedirectStandardOutput (Join-Path $logs 'validator.stdout.log') -RedirectStandardError (Join-Path $logs 'validator.stderr.log') -PassThru -WindowStyle Hidden
$process.Id | Set-Content (Join-Path $logs 'validator.pid')
Write-Output "neo-validator=$($process.Id)"
