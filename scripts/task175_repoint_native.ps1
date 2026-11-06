# Task 175: repoint neo_core::smart_contract::native -> neo_native_contracts::native
$ErrorActionPreference = 'Stop'
$root = 'D:\Git\neo-rs'
$files = Get-ChildItem -Recurse -Filter *.rs -File -Path "$root\neo-rpc", "$root\neo-node", "$root\benches-package" |
    Where-Object { $_.FullName -notmatch '\\target\\|\\neo-core\\' }

$changed = @()
foreach ($f in $files) {
    $bytes = [System.IO.File]::ReadAllBytes($f.FullName)
    $hasBom = ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF)
    $text = [System.IO.File]::ReadAllText($f.FullName)
    $orig = $text
    $text = $text.Replace('neo_core::smart_contract::native', 'neo_native_contracts::native')
    $text = $text.Replace('neo_core::NativeContract', 'neo_native_contracts::NativeContract')
    if ($text -ne $orig) {
        $enc = New-Object System.Text.UTF8Encoding($hasBom)
        [System.IO.File]::WriteAllText($f.FullName, $text, $enc)
        $changed += $f.FullName.Replace("$root\", '')
    }
}
Write-Output "Changed files: $($changed.Count)"
$changed | ForEach-Object { Write-Output "  $_" }

# Verify no leftovers (excluding guard files and neo-core)
$left = Get-ChildItem -Recurse -Filter *.rs -File -Path $root |
    Where-Object { $_.FullName -notmatch '\\target\\|\\neo-core\\|no_local_neo_vm_dependency|layer_boundary_tests' } |
    Select-String -Pattern 'neo_core::smart_contract::native|neo_core::NativeContract'
Write-Output "Leftover matches: $($left.Count)"
$left | ForEach-Object { Write-Output "  $($_.Path):$($_.LineNumber): $($_.Line.Trim())" }
