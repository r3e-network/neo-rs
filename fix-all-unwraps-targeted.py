#!/usr/bin/env python3
"""Targeted fix for files with most unwraps."""

import os
import re
import glob

def count_unwraps_in_file(file_path):
    """Count unwraps in a file, excluding test sections."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
            
        # Only count in non-test sections
        if '#[cfg(test)]' in content:
            main_content = content.split('#[cfg(test)]')[0]
            return main_content.count('.unwrap()')
        else:
            return content.count('.unwrap()')
    except:
        return 0

def fix_unwraps_in_file(file_path):
    """Fix unwraps in a specific file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Split into main and test sections
        if '#[cfg(test)]' in content:
            parts = content.split('#[cfg(test)]')
            main_content = parts[0]
            test_content = '#[cfg(test)]' + '#[cfg(test)]'.join(parts[1:])
        else:
            main_content = content
            test_content = ''
        
        # Count original unwraps
        original_count = main_content.count('.unwrap()')
        
        # Apply comprehensive replacements based on file type
        
        # General replacements
        replacements = [
            # Option unwraps
            (r'\.unwrap_or\(([^)]+)\)\.unwrap\(\)', r'.unwrap_or(\1)'),
            (r'\.unwrap_or_else\(([^)]+)\)\.unwrap\(\)', r'.unwrap_or_else(\1)'),
            (r'\.ok_or\(([^)]+)\)\.unwrap\(\)', r'.ok_or(\1)?'),
            (r'\.ok_or_else\(([^)]+)\)\.unwrap\(\)', r'.ok_or_else(\1)?'),
            
            # Iterator operations
            (r'\.next\(\)\.unwrap\(\)', '.next().ok_or_else(|| anyhow::anyhow!("Iterator exhausted"))?'),
            (r'\.first\(\)\.unwrap\(\)', '.first().ok_or_else(|| anyhow::anyhow!("Empty collection"))?'),
            (r'\.last\(\)\.unwrap\(\)', '.last().ok_or_else(|| anyhow::anyhow!("Empty collection"))?'),
            (r'\.pop\(\)\.unwrap\(\)', '.pop().ok_or_else(|| anyhow::anyhow!("Empty collection"))?'),
            
            # Parse operations
            (r'\.parse\(\)\.unwrap\(\)', '.parse()?'),
            (r'\.parse::<([^>]+)>\(\)\.unwrap\(\)', r'.parse::<\1>()?'),
            
            # Lock operations
            (r'\.lock\(\)\.unwrap\(\)', '.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?'),
            (r'\.read\(\)\.unwrap\(\)', '.read().map_err(|e| anyhow::anyhow!("RwLock poisoned: {}", e))?'),
            (r'\.write\(\)\.unwrap\(\)', '.write().map_err(|e| anyhow::anyhow!("RwLock poisoned: {}", e))?'),
            
            # Conversion operations
            (r'\.try_into\(\)\.unwrap\(\)', '.try_into()?'),
            (r'\.try_from\(([^)]+)\)\.unwrap\(\)', r'.try_from(\1)?'),
            
            # String operations
            (r'\.to_str\(\)\.unwrap\(\)', '.to_str()?'),
            (r'String::from_utf8\(([^)]+)\)\.unwrap\(\)', r'String::from_utf8(\1)?'),
            
            # HashMap/BTreeMap operations
            (r'\.get\(([^)]+)\)\.unwrap\(\)', r'.get(\1).ok_or_else(|| anyhow::anyhow!("Key not found"))?'),
            (r'\.get_mut\(([^)]+)\)\.unwrap\(\)', r'.get_mut(\1).ok_or_else(|| anyhow::anyhow!("Key not found"))?'),
            (r'\.remove\(([^)]+)\)\.unwrap\(\)', r'.remove(\1).ok_or_else(|| anyhow::anyhow!("Key not found"))?'),
        ]
        
        # File-specific replacements
        if 'vm' in file_path:
            replacements.extend([
                (r'\.as_int\(\)\.unwrap\(\)', '.as_int()?'),
                (r'\.as_integer\(\)\.unwrap\(\)', '.as_integer()?'),
                (r'\.as_bool\(\)\.unwrap\(\)', '.as_bool()?'),
                (r'\.as_bytes\(\)\.unwrap\(\)', '.as_bytes()?'),
                (r'engine\.evaluation_stack\.pop\(\)\.unwrap\(\)', 'engine.evaluation_stack.pop()?'),
                (r'engine\.evaluation_stack\.push\(([^)]+)\)\.unwrap\(\)', r'engine.evaluation_stack.push(\1)?'),
            ])
        
        if 'json' in file_path:
            replacements.extend([
                (r'\.as_str\(\)\.unwrap\(\)', '.as_str()?'),
                (r'\.as_object\(\)\.unwrap\(\)', '.as_object()?'),
                (r'\.as_array\(\)\.unwrap\(\)', '.as_array()?'),
                (r'\.as_number\(\)\.unwrap\(\)', '.as_number()?'),
            ])
        
        if 'mpt_trie' in file_path or 'trie' in file_path:
            replacements.extend([
                (r'\.root\.as_ref\(\)\.unwrap\(\)', '.root.as_ref()?'),
                (r'\.node\.as_ref\(\)\.unwrap\(\)', '.node.as_ref()?'),
            ])
        
        # Apply all replacements
        for pattern, replacement in replacements:
            main_content = re.sub(pattern, replacement, main_content)
        
        # Fix double unwraps
        main_content = re.sub(
            r'\.unwrap\(\)\s*\.unwrap\(\)',
            '??',  # Double ? operator for nested Results
            main_content
        )
        
        # Reconstruct content
        content = main_content + test_content
        
        # Count changes
        new_count = main_content.count('.unwrap()')
        changes_made = original_count - new_count
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix unwraps in targeted files."""
    # Find all Rust files
    all_files = []
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        all_files.extend(glob.glob(pattern, recursive=True))
    
    # Count unwraps in each file
    file_unwrap_counts = []
    for file_path in all_files:
        count = count_unwraps_in_file(file_path)
        if count > 0:
            file_unwrap_counts.append((file_path, count))
    
    # Sort by unwrap count
    file_unwrap_counts.sort(key=lambda x: x[1], reverse=True)
    
    print("Files with most unwraps:")
    for file_path, count in file_unwrap_counts[:10]:
        print(f"  {file_path}: {count} unwraps")
    
    # Fix top files
    total_fixes = 0
    files_fixed = 0
    
    for file_path, _ in file_unwrap_counts[:50]:  # Fix top 50 files
        fixes = fix_unwraps_in_file(file_path)
        if fixes > 0:
            print(f"Fixed {fixes} unwraps in {file_path}")
            total_fixes += fixes
            files_fixed += 1
    
    print(f"\nTotal unwraps fixed: {total_fixes} in {files_fixed} files")
    
    # Count remaining
    remaining = sum(count_unwraps_in_file(f) for f in all_files)
    print(f"Total remaining unwraps: {remaining}")

if __name__ == '__main__':
    main()