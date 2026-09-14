$id = [Security.Principal.WindowsIdentity]::GetCurrent()
$p  = New-Object Security.Principal.WindowsPrincipal($id)
@(
  "User    = $($id.Name)"
  "IsAdmin = $($p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))"
) -join "`n" | Out-File 'D:\Git\neo-rs\.cache\admin-check.txt' -Encoding utf8
