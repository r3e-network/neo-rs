$ErrorActionPreference = 'Continue'
$log = 'D:\Git\neo-rs\.cache\hklm-fix.txt'
$out = @()

$key = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment'
foreach ($pair in @(
        @{ Name = 'ProgramData'; Value = 'C:\ProgramData' },
        @{ Name = 'APPDATA';     Value = 'C:\Users\Administrator\AppData\Roaming' },
        @{ Name = 'LOCALAPPDATA'; Value = 'C:\Users\Administrator\AppData\Local' })) {
    try {
        New-ItemProperty -Path $key -Name $pair.Name -Value $pair.Value -PropertyType String -Force | Out-Null
        $out += "SET $($pair.Name) = $($pair.Value)"
    } catch {
        $out += "FAIL $($pair.Name): $_"
    }
}

foreach ($n in 'ProgramData', 'APPDATA', 'LOCALAPPDATA') {
    $v = (Get-ItemProperty -Path $key -Name $n -ErrorAction SilentlyContinue).$n
    $out += "VERIFY $n = [$v]"
}

$out -join "`n" | Out-File -FilePath $log -Encoding utf8
