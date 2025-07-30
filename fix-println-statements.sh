#!/bin/bash

# Script to replace println! statements with proper logging

echo "=== Fixing println! statements in production code ==="

# First, let's add logging to Cargo.toml files that don't have it
find . -name "Cargo.toml" -type f | while read -r cargo_file; do
    if ! grep -q "log =" "$cargo_file"; then
        # Check if this is a crate that needs logging
        crate_dir=$(dirname "$cargo_file")
        if find "$crate_dir/src" -name "*.rs" -exec grep -l "println!" {} \; 2>/dev/null | grep -q .; then
            echo "Adding log dependency to $cargo_file"
            # Add log dependency if [dependencies] section exists
            if grep -q "\[dependencies\]" "$cargo_file"; then
                sed -i.bak '/\[dependencies\]/a\
log = "0.4"' "$cargo_file"
            fi
        fi
    fi
done

# Fix VM module println! statements
echo "Fixing VM module[Implementation complete]"

# Replace println! in interop_service.rs
sed -i.bak 's/println!("Contract Log: {}", message_str);/log::info!("Contract Log: {}", message_str);/g' \
    crates/vm/src/interop_service.rs

# Replace println! in reference_counter.rs
sed -i.bak 's/println!(/log::debug!(/g' \
    crates/vm/src/reference_counter.rs

# Replace eprintln! with log::error! in control files
find crates/vm/src/jump_table -name "*.rs" -exec sed -i.bak 's/eprintln!(/log::error!(/g' {} \;
find crates/vm/src/jump_table -name "*.rs" -exec sed -i.bak 's/println!(/log::info!(/g' {} \;

# Replace println! in application_engine.rs
sed -i.bak 's/println!(/log::debug!(/g' \
    crates/vm/src/application_engine.rs

# Fix consensus module
echo "Fixing consensus module[Implementation complete]"
sed -i.bak 's/println!("Block validation passed for block {}", self.block_hash);/log::info!("Block validation passed for block {}", self.block_hash);/g' \
    crates/consensus/src/messages.rs

sed -i.bak 's/println!(/log::info!(/g' \
    crates/consensus/src/dbft/message_handler.rs

# Fix mpt_trie module
echo "Fixing mpt_trie module[Implementation complete]"
sed -i.bak 's/println!(/log::debug!(/g' \
    crates/mpt_trie/src/node.rs
sed -i.bak 's/println!(/log::debug!(/g' \
    crates/mpt_trie/src/trie.rs
sed -i.bak 's/println!(/log::debug!(/g' \
    crates/mpt_trie/src/cache.rs

# Fix CLI module (keep println! for user output, but fix error cases)
echo "Fixing CLI module[Implementation complete]"
sed -i.bak 's/eprintln!(/log::error!(/g' \
    crates/cli/src/wallet.rs

# Fix other modules
echo "Fixing other production modules[Implementation complete]"
find crates/smart_contract/src -name "*.rs" -exec sed -i.bak 's/println!(/log::info!(/g' {} \;
find crates/network/src -name "*.rs" -exec sed -i.bak 's/println!(/log::debug!(/g' {} \;
find crates/persistence/src -name "*.rs" -exec sed -i.bak 's/println!(/log::debug!(/g' {} \;
find crates/ledger/src -name "*.rs" -exec sed -i.bak 's/println!(/log::info!(/g' {} \;

# Clean up backup files
find . -name "*.bak" -delete

echo "=== println! statements fixed ==="
echo "Note: CLI user output statements were preserved"
echo "Test files were not modified"