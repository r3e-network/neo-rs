// Read and print the ACTUAL OPCODE_PRICE_TABLE from source
use std::fs;

fn main() {
    let content = fs::read_to_string("neo-core/src/smart_contract/application_engine/op_code_prices.rs")
        .expect("Failed to read file");
    
    // Extract array content
    if let Some(start) = content.find("pub const OPCODE_PRICE_TABLE") {
        let remaining = &content[start..];
        if let Some(arr_start) = remaining.find("[") {
            let arr_remaining = &remaining[arr_start + 1..];
            if let Some(arr_end) = arr_remaining.find("]") {
                let arr_content = &arr_remaining[..arr_end];
                
                // Replace newlines and spaces with empty string for splitting
                let flat = arr_content.replace('\n', "").replace(' ', "").replace('\r', "");
                
                // Split by comma and parse
                let values: Vec<i64> = flat
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.parse::<i64>().unwrap_or(0))
                    .collect();
                
                println!("OPCODE_PRICE_TABLE extracted successfully!");
                println!("Total entries: {}", values.len());
                println!();
                
                // Print specific indices
                let checks = vec![
                    ("TRY_L should be at index 60", 60),
                    ("Index where table has 32768", 54),  
                    ("Table[62]", 62),
                    ("NOP (0x21)", 33),
                    ("NZ (0xAE)", 174),
                    ("ADD (0x9E)", 158),
                    ("PICKITEM (0xCE)", 206),
                    ("APPEND (0xCF)", 207),
                ];
                
                for (desc, idx) in checks {
                    if idx < values.len() {
                        println!("{} (index {}): {}", desc, idx, values[idx]);
                    } else {
                        println!("{}: Index {} out of bounds!", desc, idx);
                    }
                }
                
                // Find where 32768 is
                if let Some(pos) = values.iter().position(|&x| x == 32768) {
                    println!("\n✅ Table value 32768 found at index: {}", pos);
                }
                
                // Find where 8192 is  
                let high_values: Vec<_> = values
                    .iter()
                    .enumerate()
                    .filter(|(_, &v)| v >= 8192 && v > 0)
                    .map(|(i, &v)| (i, v))
                    .collect();
                
                println!("\nHigh-value opcodes (≥8192):");
                for (i, v) in &high_values {
                    println!("  Index {}: {} units", i, v);
                }
            }
        }
    }
}
