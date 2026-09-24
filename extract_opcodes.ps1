# Extract opcode name → byte value mapping from Rust source
$content = Get-Content -Path "neo-vm/src/vm/opcode.rs" -Raw

# Pattern to match opcode definitions like: PUSHINT8 = 0x00,
$pattern = '([A-Z_][A-Z0-9_]*)\s*=\s*(0x[0-9a-fA-F]+)'
$matches = [System.Text.RegularExpressions.Regex]::Matches($content, $pattern)

$results = @()
foreach ($match in $matches) {
    $name = $match.Groups[1].Value
    $byte_hex = $match.Groups[2].Value.Substring(2)  # Remove "0x" prefix
    $byte_dec = [Convert]::ToInt32($byte_hex, 16)
    
    $results += [PSCustomObject]@{
        Name = $name
        ByteHex = ("0x" + $byte_hex.PadLeft(2, '0').ToUpper())
        ByteDec = $byte_dec
    }
}

# Sort by decimal byte value
$sorted = $results | Sort-Object ByteDec

# Output as Markdown table
Write-Host "## Opcode Byte Mapping Survey (Sorted by Byte Value)" -ForegroundColor Cyan
Write-Host ""
Write-Host "| Byte | Decimal | Opcode Name |" -ForegroundColor Yellow
Write-Host "|------|---------|-------------|" -ForegroundColor Yellow

foreach ($op in $sorted) {
    Write-Host "| $($op.ByteHex) | $($op.ByteDec.ToString().PadLeft(3)) | $($op.Name) |"
}

# Also output JSON for processing
Write-Host "`n---JSON---"
$sorted | ConvertTo-Json
