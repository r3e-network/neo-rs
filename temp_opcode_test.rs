// Quick test to verify Rust opcode price indices

fn main() {
    let table: [i64; 256] = [
        1, 1, 1, 1, 4, 4, 0, 0, 1, 1, 4, 1, 8, 512, 4096, 1,  // 0-15
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2,        // 16-31  
        2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,        // 32-47
        2, 2, 2, 512, 512, 512, 32768, 0, 1, 512, 4, 4, 4, 4, 4, 0,  // 48-63
        4, 4, 0, 0, 0, 2, 0, 2, 2, 0, 16, 16, 2, 2, 0, 2,      // 64-79
        16, 16, 64, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,     // 80-95
        2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,        // 96-111
        2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,        // 112-127
        2, 2, 2, 2, 2, 2, 256, 2048, 0, 2048, 2048, 2048, 2048, 0, 4, 8,  // 128-143
        8, 8, 0, 0, 0, 32, 32, 4, 4, 4, 4, 4, 8, 8, 8, 8,      // 144-159
        8, 8, 8, 8, 8, 0, 0, 2048, 2048, 2048, 2048, 16, 512, 512, 512, 512,  // 160-175
        0, 8192, 0, 4, 64, 16, 8192, 64, 8192, 8192, 8192, 16, 16, 16, 0, 0,  // 176-191
        0, 2, 2, 0, 8192, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0,     // 192-207
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,                 // 208-255 (remaining zeros)
    ];
    
    println!("Opcode Index | Price | Name");
    println!("------------|-------|------");
    
    // Key opcodes
    println!("{:>12} | {:>5} | SYSCALL (0x41)", 65);
    assert_eq!(table[65], 0, "SYSCALL at index 65 should be 0");
    
    println!("{:>12} | {:>5} | NOP (0x20)", 32);
    assert_eq!(table[32], 2, "NOP at index 32 should be 2");
    
    println!("{:>12} | {:>5} | TRY_L MAX", 66);
    assert_eq!(table[66], 32768, "Index 66 should be 32768");
    
    println!("{:>12} | {:>5} | PUSHDATA1", 12);
    assert_eq!(table[12], 8, "PUSHDATA1 at index 12 should be 8");
    
    println!("{:>12} | {:>5} | APPEND", 200);
    assert_eq!(table[200], 8192, "APPEND at index 200 should be 8192");
    
    println!("{:>12} | {:>5} | PICKITEM", 205);
    assert_eq!(table[205], 8192, "PICKITEM at index 205 should be 8192");
    
    println!("{:>12} | {:>5} | ENDTRY/L", 71);
    println!("Index 71 price: {}", table[71]);
    
    println!("{:>12} | {:>5} | ENDFINALLY", 72);
    println!("Index 72 price: {}", table[72]);
    
    // Count zeros
    let zero_count = table.iter().filter(|&&p| p == 0).count();
    println!("\nZero-priced opcodes: {}", zero_count);
    println!("Positive-priced opcodes: {}", 256 - zero_count);
}
