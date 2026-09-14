$env:PROGRAMDATA  = 'C:\ProgramData'
$env:APPDATA      = 'C:\Users\Administrator\AppData\Roaming'
$env:LOCALAPPDATA = 'C:\Users\Administrator\AppData\Local'

$log = 'D:\Git\neo-rs\.cache\envprobe-build.txt'
Set-Location 'D:\Git\neo-rs\.cache\envprobe'

& dotnet build --nologo *>&1 | Out-File -FilePath $log -Encoding utf8
"EXITCODE=$LASTEXITCODE" | Out-File -FilePath $log -Append -Encoding utf8
