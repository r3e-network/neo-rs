$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$logs = Join-Path $PSScriptRoot 'logs'
$cliDir = Join-Path $PSScriptRoot 'nodes/neo-cli/neo-cli-v3.10.2'
$goDir = Join-Path $PSScriptRoot 'nodes/neo-go'
$cli = Start-Process -FilePath (Join-Path $cliDir 'neo-cli.exe') -WorkingDirectory $cliDir -RedirectStandardOutput (Join-Path $logs 'neo-cli.stdout.log') -RedirectStandardError (Join-Path $logs 'neo-cli.stderr.log') -PassThru
$cli.Id | Set-Content (Join-Path $logs 'neo-cli.pid')
$go = Start-Process -FilePath (Join-Path $root '.t30-live/nodes/neo-go.exe') -ArgumentList @('node', '--config-file', (Join-Path $goDir 'config.yml'), '--relative-path', $goDir) -WorkingDirectory $goDir -RedirectStandardOutput (Join-Path $logs 'neo-go.stdout.log') -RedirectStandardError (Join-Path $logs 'neo-go.stderr.log') -PassThru
$go.Id | Set-Content (Join-Path $logs 'neo-go.pid')
Write-Output "neo-cli=$($cli.Id) neo-go=$($go.Id)"
