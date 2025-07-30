#!/usr/bin/env python3
"""Fix bulk unwrap() calls in production code."""

import os
import re
import glob

def fix_bulk_unwraps(file_path):
    """Fix multiple unwrap patterns in production code."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files and example files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Pattern 1: .len() comparisons that use unwrap
        pattern1 = r'\.len\(\)\s*[<>=]+\s*\d+.*\.unwrap\(\)'
        
        # Pattern 2: .is_empty() checks followed by unwrap
        pattern2 = r'if\s+!.*\.is_empty\(\).*\{[^}]*\.unwrap\(\)'
        
        # Pattern 3: .contains() checks followed by unwrap
        pattern3 = r'if\s+.*\.contains\(.*\).*\{[^}]*\.unwrap\(\)'
        
        # Pattern 4: Vec operations that unwrap
        vec_patterns = [
            (r'\.pop\(\)\.unwrap\(\)', '.pop().ok_or_else(|| anyhow::anyhow!("Collection is empty"))?'),
            (r'\.remove\(([^)]+)\)\.unwrap\(\)', r'.remove(\1)'),  # remove returns the value, no unwrap needed
            (r'\.swap_remove\(([^)]+)\)\.unwrap\(\)', r'.swap_remove(\1)'),
        ]
        
        for pattern, replacement in vec_patterns:
            if re.search(pattern, content):
                content = re.sub(pattern, replacement, content)
                changes_made += len(re.findall(pattern, original_content))
        
        # Pattern 5: HashMap/BTreeMap operations
        map_patterns = [
            (r'\.get\(([^)]+)\)\.unwrap\(\)', r'.get(\1).ok_or_else(|| anyhow::anyhow!("Key not found"))?'),
            (r'\.get_mut\(([^)]+)\)\.unwrap\(\)', r'.get_mut(\1).ok_or_else(|| anyhow::anyhow!("Key not found"))?'),
            (r'\.insert\(([^,]+),([^)]+)\)\.unwrap\(\)', r'.insert(\1,\2)'),  # insert returns Option<V>, not Result
        ]
        
        for pattern, replacement in map_patterns:
            if re.search(pattern, content):
                content = re.sub(pattern, replacement, content)
                changes_made += len(re.findall(pattern, original_content))
        
        # Pattern 6: String operations
        string_patterns = [
            (r'\.to_str\(\)\.unwrap\(\)', '.to_str().ok_or_else(|| anyhow::anyhow!("Invalid UTF-8"))?'),
            (r'\.to_string_lossy\(\)\.unwrap\(\)', '.to_string_lossy()'),  # to_string_lossy doesn't return Result
            (r'String::from_utf8\(([^)]+)\)\.unwrap\(\)', r'String::from_utf8(\1).map_err(|e| anyhow::anyhow!("Invalid UTF-8: {}", e))?'),
        ]
        
        for pattern, replacement in string_patterns:
            if re.search(pattern, content):
                content = re.sub(pattern, replacement, content)
                changes_made += len(re.findall(pattern, original_content))
        
        # Pattern 7: Path operations
        path_patterns = [
            (r'\.file_name\(\)\.unwrap\(\)', '.file_name().ok_or_else(|| anyhow::anyhow!("No file name"))?'),
            (r'\.parent\(\)\.unwrap\(\)', '.parent().ok_or_else(|| anyhow::anyhow!("No parent directory"))?'),
            (r'\.extension\(\)\.unwrap\(\)', '.extension().ok_or_else(|| anyhow::anyhow!("No file extension"))?'),
        ]
        
        for pattern, replacement in path_patterns:
            if re.search(pattern, content):
                content = re.sub(pattern, replacement, content)
                changes_made += len(re.findall(pattern, original_content))
        
        # Pattern 8: Numeric conversions
        numeric_patterns = [
            (r'\.as_u64\(\)\.unwrap\(\)', '.as_u64().ok_or_else(|| anyhow::anyhow!("Value too large for u64"))?'),
            (r'\.as_u32\(\)\.unwrap\(\)', '.as_u32().ok_or_else(|| anyhow::anyhow!("Value too large for u32"))?'),
            (r'\.to_u64\(\)\.unwrap\(\)', '.to_u64().ok_or_else(|| anyhow::anyhow!("Cannot convert to u64"))?'),
            (r'\.to_u32\(\)\.unwrap\(\)', '.to_u32().ok_or_else(|| anyhow::anyhow!("Cannot convert to u32"))?'),
        ]
        
        for pattern, replacement in numeric_patterns:
            if re.search(pattern, content):
                content = re.sub(pattern, replacement, content)
                changes_made += len(re.findall(pattern, original_content))
        
        # Pattern 9: Iterator operations
        iter_patterns = [
            (r'\.next\(\)\.unwrap\(\)', '.next().ok_or_else(|| anyhow::anyhow!("Iterator exhausted"))?'),
            (r'\.nth\(([^)]+)\)\.unwrap\(\)', r'.nth(\1).ok_or_else(|| anyhow::anyhow!("Index out of bounds"))?'),
            (r'\.find\(([^)]+)\)\.unwrap\(\)', r'.find(\1).ok_or_else(|| anyhow::anyhow!("Element not found"))?'),
        ]
        
        for pattern, replacement in iter_patterns:
            if re.search(pattern, content):
                content = re.sub(pattern, replacement, content)
                changes_made += len(re.findall(pattern, original_content))
        
        # Pattern 10: Option combinations
        option_patterns = [
            (r'\.ok_or\(([^)]+)\)\.unwrap\(\)', r'.ok_or(\1)?'),  # ok_or already returns Result
            (r'\.ok_or_else\(([^)]+)\)\.unwrap\(\)', r'.ok_or_else(\1)?'),
        ]
        
        for pattern, replacement in option_patterns:
            if re.search(pattern, content):
                content = re.sub(pattern, replacement, content)
                changes_made += len(re.findall(pattern, original_content))
        
        # Fix import if anyhow is used
        if changes_made > 0 and 'anyhow::anyhow!' in content and 'use anyhow' not in content:
            # Add anyhow import at the top if not present
            lines = content.splitlines()
            import_idx = None
            for i, line in enumerate(lines):
                if line.startswith('use '):
                    import_idx = i
                    break
            
            if import_idx is not None:
                # Check if this file likely returns Result<T, anyhow::Error>
                if 'Result<' in content and not 'use anyhow::Result;' in content:
                    lines.insert(import_idx, 'use anyhow::Result;')
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            print(f"Fixed {changes_made} unwraps in {file_path}")
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix bulk unwraps."""
    total_fixes = 0
    
    # Process all Rust files in crates and node directories
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_bulk_unwraps(file_path)
                total_fixes += fixes
    
    print(f"\nTotal bulk unwraps fixed: {total_fixes}")

if __name__ == '__main__':
    main()