$env:PROGRAMDATA  = 'C:\ProgramData'
$env:APPDATA      = 'C:\Users\Administrator\AppData\Roaming'
$env:LOCALAPPDATA = 'C:\Users\Administrator\AppData\Local'
$env:USERPROFILE  = 'C:\Users\Administrator'

Set-Location 'D:\Git\neo-rs\tools\csharp-vm-runner'

# Redirect via Start-Process so the child's own stdout/stderr go straight to disk.
$p = Start-Process -FilePath 'C:\Program Files\dotnet\dotnet.exe' `
     -ArgumentList 'build','--nologo','-v','q' `
     -WorkingDirectory 'D:\Git\neo-rs\tools\csharp-vm-runner' `
     -RedirectStandardOutput 'D:\Git\neo-rs\.cache\build-out.txt' `
     -RedirectStandardError  'D:\Git\neo-rs\.cache\build-err.txt' `
     -NoNewWindow -Wait -PassThru
"EXIT=$($p.ExitCode)" | Out-File 'D:\Git\neo-rs\.cache\build-exit.txt' -Encoding utf8
