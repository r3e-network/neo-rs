# Fix missing machine-level environment variables for NuGet.
# If started without elevation, relaunch itself through UAC.

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    $argList = @(
        '-NoProfile'
        '-ExecutionPolicy', 'Bypass'
        '-NoExit'
        '-File', "`"$PSCommandPath`""
    )
    try {
        Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList $argList -ErrorAction Stop | Out-Null
        Write-Host '已请求管理员权限。请在新窗口中查看结果。' -ForegroundColor Yellow
    }
    catch {
        Write-Error '无法获得管理员权限。请右键 PowerShell，选择“以管理员身份运行”。'
    }
    exit
}

$key = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment'

Set-ItemProperty -Path $key -Name 'ProgramData' -Value 'C:\ProgramData' -Type String -ErrorAction Stop
Set-ItemProperty -Path $key -Name 'APPDATA' -Value 'C:\Users\Administrator\AppData\Roaming' -Type String -ErrorAction Stop
Set-ItemProperty -Path $key -Name 'LOCALAPPDATA' -Value 'C:\Users\Administrator\AppData\Local' -Type String -ErrorAction Stop

foreach ($n in 'ProgramData','APPDATA','LOCALAPPDATA') {
    $v = (Get-ItemProperty -Path $key -Name $n).$n
    Write-Host ("{0} = {1}" -f $n, $v)
}

Write-Host ''
Write-Host 'Machine environment fixed. Restart WorkBuddy or log off and log on.' -ForegroundColor Green
Write-Host 'Then run: dotnet build D:\Git\neo-rs\tools\csharp-vm-runner' -ForegroundColor Cyan
