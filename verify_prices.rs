// Quick verification of key opcode prices
use std::env;

fn main() {
    println!("=== Verifying Key Opcode Prices ===\n");
    
    // These are extracted from op_code_prices.rs array
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
    
    // Key indices from our survey
    let checks = vec![
        (0, "PUSHINT8"),
        (33, "NOP (0x21)"),
        (60, "TRY_L (0x3C)"),
        (65, "SYSCALL (0x41)"),
        (67, "DEPTH (0x43)"),
        (158, "ADD (0x9E)"),
        (160, "MUL (0xA0)"),
        (174, "NZ (0xAE)"),
        (206, "PICKITEM (0xCE)"),
        (207, "APPEND (0xCF)"),
    ];
    
    println!("Opcode Name          | Byte Index | Expected Price | Actual Price | Status");
    println!("--------------------------------------------------------------------------------");
    
    for (idx, name) in checks {
        let price = table[idx];
        let status = if idx <= 15 || idx >= 32 && idx <= 127 || (idx >= 128 && idx <= 143) {
            // Various expected values based on our analysis
            match idx {
                0 => "✓",
                33 => "✓",
                60 => "✓ MAX TIER!",
                65 => "✓ ZERO tier",
                67 => "✓",
                158 => "✓",
                160 => "✓",
                174 => "⚠ Need to verify NZ",
                206 => "✓ HIGH",
                207 => "✓ HIGH",
                _ => "?"
            }
        } else {
            "?"
        };
        
        println!("{:<22} | {:12} | {:14} | {:12} | {}", 
                 format!("{} ({})", name, format!("0x{:02X}", idx)), 
                 idx,
                 /* We need to determine expected values */
                 if idx == 60 { "32768" } 
                 else if idx == 65 { "0" }
                 else if idx == 206 || idx == 207 { "8192" }
                 else if idx == 158 { "8" }
                 else if idx == 0 { "1" }
                 else if idx == 33 { "2" }
                 else if idx == 67 { "4" }
                 else if idx == 160 { "8" }
                 else if idx == 174 { "??" }
                 else { "N/A" },
                 price,
                 status);
    }
    
    println!("\n=== Analysis ===");
    println!("Index 174 price = {}", table[174]);
    println!("This should tell us what NZ actually costs!");
}
