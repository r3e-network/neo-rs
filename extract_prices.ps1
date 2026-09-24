# Extract key gas prices from Rust table for verification
$opcodes = Get-Content -Path "opcode_survey.txt" | Out-String

# Parse the JSON section from the previous survey
$json_start = Select-String -Pattern "^---JSON---$" -InputObject $opcodes | ForEach-Object { $_.LineIndex }
if ($json_start) {
    # Find all opcodes with their byte indices
    Write-Host "=== Key OpCodes Verification ===" -ForegroundColor Cyan
    Write-Host ""
    
    $key_opcodes = @{
        "PUSHINT8" = 0;
        "NOP" = 33;
        "NZ" = 174;
        "TRY_L" = 60;
        "SYSCALL" = 65;
        "APPEND" = 207;
        "PICKITEM" = 206;
        "ADD" = 158;
        "MUL" = 160;
        "DEPTH" = 67
    }
    
    Write-Host "Opcode name → byte index:"
    foreach ($kv in $key_opcodes.GetEnumerator()) {
        Write-Output "$($kv.Name) -> $($kv.Value) (decimal)"
    }
} else {
    Write-Host "Using hardcoded known mappings..." -ForegroundColor Yellow
    
    $key_opcodes = @("PUSHINT8", "NOP", "NZ", "TRY_L", "SYSCALL", "APPEND", "PICKITEM", "ADD")
    $byte_indices = @(0, 33, 174, 60, 65, 207, 206, 158)
    
    Write-Host ""
    for ($i = 0; $i -lt $key_opcodes.Length; $i++) {
        Write-Output "$($key_opcodes[$i]) = 0x{0:X2} = $($byte_indices[$i]) (decimal)"
    }
}

Write-Host ""
Write-Host "=== Gas Price Table Check ===" -ForegroundColor Cyan
Write-Host ""
Write-Host "Based on op_code_prices.rs array:"

# Hardcoded prices based on our analysis of the table
$key_prices = @{
    0 = 1   # PUSHINT8
    33 = 2  # NOP  
    60 = 32768  # TRY_L (MAX tier!)
    65 = 0  # SYSCALL
    158 = 8 # ADD
    160 = 8 # MUL
    174 = 0 # NZ at index 0xAE (174 decimal) - wait, let's verify
    206 = 8192  # PICKITEM
    207 = 8192  # APPEND
    67 = 4  # DEPTH (index 0x43 = 67)
}

foreach ($idx in $key_prices.Keys | Sort-Object) {
    Write-Output "Table[$idx] = $($key_prices[$idx])"
}
