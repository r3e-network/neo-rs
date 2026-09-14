b = open('ci_script.bin', 'rb').read()
opnames = {}
with open('neo-vm/src/vm/opcode.rs') as f:
    for line in f:
        if '=' in line and 'operand_size' in line:
            parts = line.strip().split('=')
            name = parts[0].strip()
            val_str = parts[1].split(',')[0].strip()
            if val_str.startswith('0x'):
                val = int(val_str, 16)
                opnames[val] = name

i = 100
while i < 250 and i < len(b):
    op = b[i]
    name = opnames.get(op, f'0x{op:02x}')
    if op == 0x57: # INITSLOT
        print(f'{i:04d}: {name} locals={b[i+1]} args={b[i+2]}')
        i += 3
    elif op in (0x22, 0x24, 0x26, 0x28, 0x2A, 0x34): # 1-byte
        rel = int.from_bytes(b[i+1:i+2], 'little', signed=True)
        print(f'{i:04d}: {name} {rel} -> {i + rel}')
        i += 2
    elif op in (0x23, 0x25, 0x27, 0x29, 0x2B, 0x35): # 4-byte
        rel = int.from_bytes(b[i+1:i+5], 'little', signed=True)
        print(f'{i:04d}: {name} {rel} -> {i + rel}')
        i += 5
    elif op == 0x41: # SYSCALL
        print(f'{i:04d}: {name} 0x{b[i+1:i+5].hex()}')
        i += 5
    elif op == 0x0c: # PUSHDATA1
        n = b[i+1]
        print(f'{i:04d}: {name} len={n} {b[i+2:i+2+n]}')
        i += 2 + n
    else:
        print(f'{i:04d}: {name}')
        i += 1
