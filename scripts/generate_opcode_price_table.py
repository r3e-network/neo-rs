import re
import json
import subprocess
from pathlib import Path

metadata = json.loads(
    subprocess.check_output(["cargo", "metadata", "--format-version", "1"], text=True)
)
neo_vm_rs = next(
    package for package in metadata["packages"] if package["name"] == "neo-vm-rs"
)
opcode_path = Path(neo_vm_rs["manifest_path"]).parent / "src" / "vm" / "opcode.rs"

with opcode_path.open("r") as f:
    rust_code = f.read()

opcode_values = {}
for line in rust_code.split('\n'):
    line = line.strip()
    if '=' in line and ',' in line and '0x' in line:
        parts = line.split('=')
        name = parts[0].strip()
        val_str = parts[1].split(',')[0].strip()
        try:
            val = int(val_str, 16)
            opcode_values[name] = val
        except ValueError:
            pass

import urllib.request
url = "https://raw.githubusercontent.com/neo-project/neo/master/src/Neo/SmartContract/ApplicationEngine.OpCodePrices.cs"
csharp_code = urllib.request.urlopen(url).read().decode('utf-8')

prices = [0] * 256
for line in csharp_code.split('\n'):
    line = line.strip()
    if line.startswith('[OpCode.') and ']' in line and '=' in line:
        m = re.match(r'\[OpCode\.([A-Z0-9_]+)\]\s*=\s*(.+),', line)
        if m:
            name = m.group(1)
            expr = m.group(2)
            if '<<' in expr:
                parts = expr.split('<<')
                val = int(parts[0].strip()) << int(parts[1].strip())
            else:
                val = int(expr.strip())

            if name in opcode_values:
                prices[opcode_values[name]] = val
            else:
                print(f"// Warning: {name} not found in Rust opcodes")

out = "        "
for i in range(256):
    out += f"{prices[i]}, "
    if (i + 1) % 30 == 0:
        out += "\n        "
print(out.strip())
