$ErrorActionPreference = 'Stop'
$log = 'D:\Git\neo-rs\.cache\setenv-result.txt'
$lines = @()
$lines += "Before: PROGRAMDATA=[$([Environment]::GetEnvironmentVariable('PROGRAMDATA','User'))] APPDATA=[$([Environment]::GetEnvironmentVariable('APPDATA','User'))]"
try {
  [Environment]::SetEnvironmentVariable('PROGRAMDATA', 'C:\ProgramData', 'User')
  [Environment]::SetEnvironmentVariable('APPDATA', 'C:\Users\Administrator\AppData\Roaming', 'User')
  $lines += "Set OK"
} catch {
  $lines += "Set FAILED: $_"
}
$lines += "After: PROGRAMDATA=[$([Environment]::GetEnvironmentVariable('PROGRAMDATA','User'))] APPDATA=[$([Environment]::GetEnvironmentVariable('APPDATA','User'))]"
$lines += "Machine PROGRAMDATA=[$([Environment]::GetEnvironmentVariable('PROGRAMDATA','Machine'))]"
$lines | Out-File -FilePath $log -Encoding utf8
