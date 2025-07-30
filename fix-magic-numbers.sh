#!/bin/bash

# Script to replace magic numbers with named constants

echo "=== Replacing magic numbers with named constants ==="

# Add use statement for constants in files that need them
add_constants_import() {
    local file=$1
    local module_path=$2
    
    # Check if the file already imports constants
    if ! grep -q "use.*constants" "$file"; then
        # Add the import after other use statements or at the top
        if grep -q "^use " "$file"; then
            # Find the last use statement and add after it
            sed -i.bak "/^use .*;\$/a\\
use $module_path;" "$file"
        else
            # Add at the beginning of the file after module docs
            sed -i.bak "1,/^\/\/\//! { /^$/a\\
use $module_path;\\
" "$file"
        fi
    fi
}

# Replace time constants
echo "Replacing time constants[Implementation complete]"
find crates -name "*.rs" -type f | while read -r file; do
    # Skip test files
    if [[ "$file" == *"/tests/"* ]] || [[ "$file" == *"_test.rs" ]]; then
        continue
    fi
    
    # Replace 3600 (1 hour in seconds)
    if grep -q "3600" "$file"; then
        add_constants_import "$file" "crate::constants::SECONDS_PER_HOUR"
        sed -i.bak 's/\b3600\b/SECONDS_PER_HOUR/g' "$file"
    fi
    
    # Replace 15000 (15 seconds in milliseconds)
    if grep -q "15000" "$file"; then
        add_constants_import "$file" "crate::constants::MILLISECONDS_PER_BLOCK"
        sed -i.bak 's/\b15000\b/MILLISECONDS_PER_BLOCK/g' "$file"
    fi
    
    # Replace 1468595301000 (genesis timestamp)
    if grep -q "1468595301000" "$file"; then
        add_constants_import "$file" "crate::constants::GENESIS_TIMESTAMP_MS"
        sed -i.bak 's/\b1468595301000\b/GENESIS_TIMESTAMP_MS/g' "$file"
    fi
done

# Replace size constants
echo "Replacing size constants[Implementation complete]"
find crates -name "*.rs" -type f | while read -r file; do
    # Skip test files
    if [[ "$file" == *"/tests/"* ]] || [[ "$file" == *"_test.rs" ]]; then
        continue
    fi
    
    # Replace 262144 (256KB)
    if grep -q "262144" "$file"; then
        add_constants_import "$file" "crate::constants::MAX_BLOCK_SIZE"
        sed -i.bak 's/\b262144\b/MAX_BLOCK_SIZE/g' "$file"
    fi
    
    # Replace 1048576 (1MB)
    if grep -q "1048576" "$file"; then
        add_constants_import "$file" "crate::constants::ONE_MEGABYTE"
        sed -i.bak 's/\b1048576\b/ONE_MEGABYTE/g' "$file"
    fi
    
    # Replace 1000 for channel sizes
    if grep -q "capacity: 1000" "$file"; then
        add_constants_import "$file" "crate::constants::DEFAULT_CHANNEL_SIZE"
        sed -i.bak 's/capacity: 1000/capacity: DEFAULT_CHANNEL_SIZE/g' "$file"
    fi
    
    # Replace 5000 for timeouts
    if grep -q "timeout.*5000" "$file"; then
        add_constants_import "$file" "crate::constants::DEFAULT_TIMEOUT_MS"
        sed -i.bak 's/timeout.*5000/timeout: DEFAULT_TIMEOUT_MS/g' "$file"
    fi
done

# Fix specific files with known magic numbers
echo "Fixing specific files[Implementation complete]"

# consensus/src/messages.rs
if [ -f "crates/consensus/src/messages.rs" ]; then
    sed -i.bak '1i\
use neo_core::constants::SECONDS_PER_HOUR;\
' crates/consensus/src/messages.rs
fi

# Clean up backup files
find . -name "*.bak" -delete

echo "=== Magic numbers replaced with constants ==="
echo "Note: Some context-specific numbers may still need manual review"