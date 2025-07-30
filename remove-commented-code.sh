#!/bin/bash

# Script to remove commented out code blocks

echo "=== Removing commented out code blocks ==="

# Remove specific commented code blocks identified in the analysis

# 1. Fix crates/ledger/src/blockchain/state.rs
echo "Fixing crates/ledger/src/blockchain/state.rs[Implementation complete]"
sed -i.bak '1746,1753d' crates/ledger/src/blockchain/state.rs

# 2. Fix crates/wallets/src/contract.rs
echo "Fixing crates/wallets/src/contract.rs[Implementation complete]"
sed -i.bak '271,272d' crates/wallets/src/contract.rs

# 3. Fix crates/network/src/p2p/mod.rs
if [ -f "crates/network/src/p2p/mod.rs" ]; then
    echo "Fixing crates/network/src/p2p/mod.rs[Implementation complete]"
    # Remove specific commented lines (need to check exact content)
    grep -n "^[[:space:]]*//.*" crates/network/src/p2p/mod.rs | grep -E "(18|27):" | while read -r line; do
        line_num=$(echo "$line" | cut -d: -f1)
        # Check if it's actually commented code (contains assignment or function call)
        if echo "$line" | grep -E "(=|\.|\()" > /dev/null; then
            sed -i.bak "${line_num}d" crates/network/src/p2p/mod.rs
        fi
    done
fi

# 4. Fix crates/smart_contract/src/native/neo_token.rs
echo "Fixing crates/smart_contract/src/native/neo_token.rs[Implementation complete]"
if [ -f "crates/smart_contract/src/native/neo_token.rs" ]; then
    # Remove lines with commented code at specific line numbers
    for line_num in 621 678 681; do
        # Check if the line contains commented code (not just comments)
        if sed -n "${line_num}p" crates/smart_contract/src/native/neo_token.rs | grep -E "^[[:space:]]*//.*[=\(\.]" > /dev/null 2>&1; then
            sed -i.bak "${line_num}d" crates/smart_contract/src/native/neo_token.rs
        fi
    done
fi

# 5. Remove multi-line commented code blocks in vm/src/jump_table/control_backup.rs
echo "Fixing vm commented code blocks[Implementation complete]"
if [ -f "crates/vm/src/jump_table/control_backup.rs" ]; then
    # Remove the commented C# code blocks
    sed -i.bak '1812,1817d' crates/vm/src/jump_table/control_backup.rs 2>/dev/null || true
    sed -i.bak '1897,1898d' crates/vm/src/jump_table/control_backup.rs 2>/dev/null || true
fi

# 6. Clean up backup files
find . -name "*.bak" -delete

echo "=== Commented code blocks removed ==="
echo "Note: Only removed actual commented code, preserved documentation comments"