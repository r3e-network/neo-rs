$log = 'D:\Git\neo-rs\.cache\cs-build.txt'
$env:PROGRAMDATA  = 'C:\ProgramData'
$env:APPDATA      = 'C:\Users\Administrator\AppData\Roaming'
$env:LOCALAPPDATA = 'C:\Users\Administrator\AppData\Local'

$sb = New-Object System.Text.StringBuilder
[void]$sb.AppendLine("PSVersion=$($PSVersionTable.PSVersion)")
[void]$sb.AppendLine("Get-Command dotnet -> " + (Get-Command dotnet -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source))
[void]$sb.AppendLine("Test-Path dotnet.exe -> " + (Test-Path 'C:\Program Files\dotnet\dotnet.exe'))
$sb.ToString() | Out-File -FilePath $log -Encoding utf8
