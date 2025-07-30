#!/usr/bin/env python3
"""
Fix critical unwrap() calls in production code
"""

import os
import re
import sys

# Define replacements for common patterns
REPLACEMENTS = {
    # Lock unwraps
    r'\.lock\(\)\.unwrap\(\)': '.lock().map_err(|_| "Failed to acquire lock")?',
    
    # Parse unwraps
    r'\.parse\(\)\.unwrap\(\)': '.parse()?',
    r'\.parse::<([^>]+)>\(\)\.unwrap\(\)': '.parse::<\1>()?',
    
    # SystemTime unwraps
    r'SystemTime::now\(\)\.duration_since\(.*?\)\.unwrap\(\)': 
        'SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()',
    
    # Option unwraps that can use unwrap_or_default
    r'\.get\([^)]+\)\.cloned\(\)\.unwrap\(\)': '.get(\1).cloned().unwrap_or_default()',
    
    # Vector/slice access
    r'\.get\(0\)\.unwrap\(\)': '.first().ok_or("Empty collection")?',
    r'\.get\(([^)]+)\)\.unwrap\(\)': '.get(\1).ok_or("Index out of bounds")?',
    
    # HashMap/BTreeMap operations
    r'\.insert\([^)]+\)\.unwrap\(\)': '.insert(\1)',
    r'\.remove\([^)]+\)\.unwrap\(\)': '.remove(\1).ok_or("Key not found")?',
}

def fix_unwraps_in_file(filepath):
    """Fix unwrap() calls in a single file."""
    if not filepath.endswith('.rs'):
        return 0
    
    # Skip test files
    if 'test' in filepath or 'tests' in filepath or 'bench' in filepath:
        return 0
    
    try:
        with open(filepath, 'r') as f:
            content = f.read()
    except:
        return 0
    
    original = content
    changes = 0
    
    # Apply replacements
    for pattern, replacement in REPLACEMENTS.items():
        new_content = re.sub(pattern, replacement, content)
        if new_content != content:
            changes += content.count(pattern)
            content = new_content
    
    # Fix simple unwraps in error-returning functions
    if ' -> Result<' in content or ' -> anyhow::Result<' in content:
        # Replace .unwrap() with ? for Results
        content = re.sub(r'(\w+)\.unwrap\(\)', r'\1?', content)
    
    # Write back if changed
    if content != original:
        with open(filepath, 'w') as f:
            f.write(content)
        print(f"Fixed {changes} unwrap() calls in {filepath}")
        return changes
    
    return 0

def main():
    """Main function to process all Rust files."""
    total_fixed = 0
    
    # Priority modules to fix
    priority_dirs = [
        'crates/vm/src',
        'crates/network/src',
        'crates/consensus/src',
        'crates/ledger/src',
        'crates/persistence/src',
        'crates/smart_contract/src',
    ]
    
    for module_dir in priority_dirs:
        if not os.path.exists(module_dir):
            continue
            
        print(f"\nProcessing {module_dir}"Implementation complete"")
        for root, dirs, files in os.walk(module_dir):
            for file in files:
                if file.endswith('.rs'):
                    filepath = os.path.join(root, file)
                    fixed = fix_unwraps_in_file(filepath)
                    total_fixed += fixed
    
    print(f"\nTotal unwrap() calls fixed: {total_fixed}")

if __name__ == "__main__":
    main()