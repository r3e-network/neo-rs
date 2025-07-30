#!/bin/bash

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo "=== Neo-RS Code Duplication Check ==="
echo "Timestamp: $(date)"
echo

# Function to check for duplicate imports
check_duplicate_imports() {
    echo -e "${BLUE}=== Checking for Duplicate Imports ===${NC}"
    
    local duplicates=0
    
    # Find files with duplicate imports
    for file in $(find crates/ node/src/ -name "*.rs" -type f); do
        # Check for duplicate use statements
        local dup_count=$(grep "^use " "$file" 2>/dev/null | sort | uniq -d | wc -l)
        if [ "$dup_count" -gt 0 ]; then
            echo -e "${RED}Duplicate imports in $file:${NC}"
            grep "^use " "$file" | sort | uniq -d | while read -r line; do
                echo "  - $line"
            done
            duplicates=$((duplicates + dup_count))
        fi
    done
    
    if [ "$duplicates" -eq 0 ]; then
        echo -e "${GREEN}✓ No duplicate imports found${NC}"
    else
        echo -e "${RED}✗ Found $duplicates duplicate imports${NC}"
    fi
    echo
}

# Function to check for duplicate constants
check_duplicate_constants() {
    echo -e "${BLUE}=== Checking for Duplicate Constants ===${NC}"
    
    # Create a temporary file to store constants
    local temp_file=$(mktemp)
    
    # Find all const declarations
    find crates/ node/src/ -name "*.rs" -type f -exec grep -H "^pub const\|^const" {} \; | \
        grep -v "test" | \
        sed 's/:.*//' | \
        sort | uniq -c | \
        awk '$1 > 1 {print $2}' > "$temp_file"
    
    local dup_count=$(wc -l < "$temp_file")
    
    if [ "$dup_count" -eq 0 ]; then
        echo -e "${GREEN}✓ No duplicate constant names found${NC}"
    else
        echo -e "${YELLOW}⚠ Found files with potentially duplicate constants:${NC}"
        cat "$temp_file"
    fi
    
    rm -f "$temp_file"
    echo
}

# Function to check for duplicate functions
check_duplicate_functions() {
    echo -e "${BLUE}=== Checking for Duplicate Function Signatures ===${NC}"
    
    # Use Python for more accurate function signature detection
    python3 -c "
import os
import re
from collections import defaultdict

function_sigs = defaultdict(list)

for root, dirs, files in os.walk('crates'):
    dirs[:] = [d for d in dirs if d not in ['tests', 'test', 'examples', 'benches', '.git', 'target']]
    
    for file in files:
        if file.endswith('.rs'):
            filepath = os.path.join(root, file)
            try:
                with open(filepath, 'r') as f:
                    content = f.read()
                    
                # Find function signatures
                # Match pub fn, fn, pub async fn, async fn
                pattern = r'^\s*(pub\s+)?(async\s+)?fn\s+(\w+)\s*\([^)]*\)'
                
                for match in re.finditer(pattern, content, re.MULTILINE):
                    func_name = match.group(3)
                    full_sig = match.group(0).strip()
                    function_sigs[func_name].append((filepath, full_sig))
                    
            except Exception as e:
                pass

# Find duplicates
duplicates = 0
for func_name, locations in function_sigs.items():
    if len(locations) > 3:  # Only report if function appears in more than 3 files
        print(f'Function \"{func_name}\" appears in {len(locations)} files:')
        for filepath, sig in locations[:5]:  # Show first 5
            print(f'  - {filepath}')
        if len(locations) > 5:
            print(f'  [Implementation complete] and {len(locations) - 5} more files')
        print()
        duplicates += 1

if duplicates == 0:
    print('✓ No excessive function duplication found')
else:
    print(f'⚠ Found {duplicates} functions that appear in many files')
"
    echo
}

# Function to check for duplicate code blocks
check_code_duplication() {
    echo -e "${BLUE}=== Checking for Duplicate Code Blocks ===${NC}"
    
    # Use a simple approach to find similar code blocks
    python3 -c "
import os
import re
from collections import defaultdict
import hashlib

def normalize_code(code):
    # Remove comments and normalize whitespace
    code = re.sub(r'//.*', '', code)
    code = re.sub(r'/\*.*?\*/', '', code, flags=re.DOTALL)
    code = re.sub(r'\s+', ' ', code)
    return code.strip()

def extract_code_blocks(content):
    # Extract function bodies
    blocks = []
    
    # Find function definitions and their bodies
    pattern = r'(fn\s+\w+[^{]*\{[^{}]*(?:\{[^{}]*\}[^{}]*)*\})'
    
    for match in re.finditer(pattern, content, re.DOTALL):
        block = match.group(1)
        if len(block) > 100:  # Only consider blocks larger than 100 chars
            normalized = normalize_code(block)
            if len(normalized) > 50:  # After normalization
                blocks.append(normalized)
    
    return blocks

code_hashes = defaultdict(list)

for root, dirs, files in os.walk('crates'):
    dirs[:] = [d for d in dirs if d not in ['tests', 'test', 'examples', 'benches', '.git', 'target']]
    
    for file in files:
        if file.endswith('.rs'):
            filepath = os.path.join(root, file)
            try:
                with open(filepath, 'r') as f:
                    content = f.read()
                
                blocks = extract_code_blocks(content)
                
                for block in blocks:
                    # Create hash of normalized code
                    block_hash = hashlib.md5(block.encode()).hexdigest()
                    code_hashes[block_hash].append((filepath, block[:100] + '[Implementation complete]'))
                    
            except Exception as e:
                pass

# Find duplicates
duplicates = 0
for block_hash, locations in code_hashes.items():
    if len(locations) > 1:
        print(f'Duplicate code block found in {len(locations)} locations:')
        for filepath, preview in locations:
            print(f'  - {filepath}')
        print(f'  Preview: {locations[0][1]}')
        print()
        duplicates += 1
        if duplicates >= 10:  # Limit output
            print('[Implementation complete] (showing first 10 duplicates)')
            break

if duplicates == 0:
    print('✓ No significant code block duplication found')
else:
    print(f'⚠ Found {duplicates} duplicate code blocks')
"
    echo
}

# Function to check for duplicate struct definitions
check_duplicate_structs() {
    echo -e "${BLUE}=== Checking for Duplicate Struct Definitions ===${NC}"
    
    python3 -c "
import os
import re
from collections import defaultdict

struct_defs = defaultdict(list)

for root, dirs, files in os.walk('crates'):
    dirs[:] = [d for d in dirs if d not in ['tests', 'test', 'examples', 'benches', '.git', 'target']]
    
    for file in files:
        if file.endswith('.rs'):
            filepath = os.path.join(root, file)
            try:
                with open(filepath, 'r') as f:
                    content = f.read()
                    
                # Find struct definitions
                pattern = r'^\s*(pub\s+)?struct\s+(\w+)'
                
                for match in re.finditer(pattern, content, re.MULTILINE):
                    struct_name = match.group(2)
                    struct_defs[struct_name].append(filepath)
                    
            except Exception as e:
                pass

# Find duplicates
duplicates = 0
for struct_name, locations in struct_defs.items():
    if len(locations) > 1 and struct_name not in ['Error', 'Config', 'Options', 'Context']:
        print(f'Struct \"{struct_name}\" defined in multiple files:')
        for filepath in locations:
            print(f'  - {filepath}')
        print()
        duplicates += 1

if duplicates == 0:
    print('✓ No duplicate struct definitions found')
else:
    print(f'⚠ Found {duplicates} structs defined in multiple files')
"
    echo
}

# Function to check for duplicate trait implementations
check_duplicate_impls() {
    echo -e "${BLUE}=== Checking for Duplicate Trait Implementations ===${NC}"
    
    python3 -c "
import os
import re
from collections import defaultdict

impl_patterns = defaultdict(list)

for root, dirs, files in os.walk('crates'):
    dirs[:] = [d for d in dirs if d not in ['tests', 'test', 'examples', 'benches', '.git', 'target']]
    
    for file in files:
        if file.endswith('.rs'):
            filepath = os.path.join(root, file)
            try:
                with open(filepath, 'r') as f:
                    content = f.read()
                    
                # Find impl blocks
                pattern = r'impl\s+(?:<[^>]+>\s+)?(\w+)\s+for\s+(\w+)'
                
                for match in re.finditer(pattern, content):
                    trait_name = match.group(1)
                    type_name = match.group(2)
                    impl_key = f'{trait_name} for {type_name}'
                    impl_patterns[impl_key].append(filepath)
                    
            except Exception as e:
                pass

# Find duplicates
duplicates = 0
for impl_key, locations in impl_patterns.items():
    if len(locations) > 1:
        print(f'Duplicate implementation \"{impl_key}\" found in:')
        for filepath in locations:
            print(f'  - {filepath}')
        print()
        duplicates += 1

if duplicates == 0:
    print('✓ No duplicate trait implementations found')
else:
    print(f'✗ Found {duplicates} duplicate trait implementations')
"
    echo
}

# Main execution
check_duplicate_imports
check_duplicate_constants
check_duplicate_functions
check_duplicate_structs
check_code_duplication
check_duplicate_impls

echo -e "${BLUE}=== Duplication Check Complete ===${NC}"
echo "To fix duplications, run: ./fix-duplications.py"