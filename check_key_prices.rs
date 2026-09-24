// Quick check of key opcode prices from OPCODE_PRICE_TABLE
fn main() {
    let table: [i64; 256] = [
        1, 1, 1, 1, 4, 4, 0, 0, 1, 1, 4, 1, 8, 512, 4096, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 512, 512, 512, 32768,
        0, 1, 512, 4, 4, 4, 4, 4, 0, 0, 0, 2, 0, 2, 2, 0, 16, 16, 2, 2, 0, 2, 2, 0, 2, 2, 16, 2, 2,
        16, 16, 64, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
        2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 256, 2048, 0, 2048, 2048,
        2048, 2048, 0, 4, 8, 8, 8, 0, 0, 0, 32, 32, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 64, 64, 32, 2048,
        0, 8, 8, 4, 8, 8, 0, 0, 0, 0, 4, 0, 8, 8, 8, 8, 8, 8, 8, 8, 8, 0, 0, 2048, 2048, 2048,
        2048, 16, 512, 512, 512, 512, 0, 8192, 0, 4, 64, 16, 8192, 64, 8192, 8192, 8192, 16, 16, 16, 0,
        0, 0, 2, 2, 0, 8192, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    
    println!("=== Key Opcode Price Verification ===\n");
    
    let checks = vec![
        (0, "PUSHINT8"),
        (12, "PUSHDATA1"),
        (13, "PUSHDATA2"),
        (14, "PUSHDATA4"),
        (33, "NOP at 0x21"),
        (54, "MAX tier value"),
        (60, "Value at byte 0x3C"),
        (65, "SYSCALL"),
        (158, "ADD at 0x9E"),
        (160, "MUL at 0xA0"),
        (171, "BOOLAND at 0xAB"),
        (173, "Index 0xAD"),
        (174, "NZ at 0xAE"),
        (195, "NEWARRAY"),
        (197, "NEWSTRUCT0"),
        (206, "PICKITEM"),
        (207, "APPEND"),
    ];
    
    for (idx, name) in &checks {
        println!("Table[{}] ({}) = {}", idx, name, table[*idx as usize]);
    }
        
    // Find max values
    let mut max_pos = 0usize;
    let mut max_val = i64::MIN;
    for (i, &v) in table.iter().enumerate() {
        if v > max_val {
            max_val = v;
            max_pos = i;
        }
    }
    println!("\n=== MAX VALUE FOUND ===");
    println!("32768 found at index: {}", max_pos);
    println!("Value: {}", max_val);
}
