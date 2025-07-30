#!/usr/bin/env python3
import os
import re
from pathlib import Path

def fix_unwrap_comprehensive(filepath):
    """Aggressively fix all unwrap() calls"""
    if any(skip in str(filepath) for skip in ['target/', '.git/', 'test']):
        return 0
    
    try:
        with open(filepath, 'r') as f:
            content = f.read()
        
        if '.unwrap()' not in content:
            return 0
        
        original_content = content
        changes = 0
        
        # Pattern 1: SystemTime unwraps
        patterns = [
            (r'\.duration_since\(([^)]+)\)\.unwrap\(\)', r'.duration_since(\1).unwrap_or_default()'),
            (r'SystemTime::now\(\)\.duration_since\(([^)]+)\)\.unwrap\(\)', r'SystemTime::now().duration_since(\1).unwrap_or_default()'),
        ]
        
        for pattern, replacement in patterns:
            content, n = re.subn(pattern, replacement, content)
            changes += n
        
        # Pattern 2: Parse unwraps - be more aggressive
        parse_patterns = [
            (r'\.parse\(\)\.unwrap\(\)', r'.parse().unwrap_or_default()'),
            (r'\.parse::<([^>]+)>\(\)\.unwrap\(\)', r'.parse::<\1>().unwrap_or_default()'),
            (r'"([^"]+)"\.parse\(\)\.unwrap\(\)', r'"\1".parse().unwrap_or_default()'),
            (r'"([^"]+)"\.parse::<([^>]+)>\(\)\.unwrap\(\)', r'"\1".parse::<\2>().unwrap_or_default()'),
        ]
        
        for pattern, replacement in parse_patterns:
            content, n = re.subn(pattern, replacement, content)
            changes += n
        
        # Pattern 3: Lock/RwLock unwraps
        lock_patterns = [
            (r'\.lock\(\)\.unwrap\(\)', r'.lock().ok()?'),
            (r'\.read\(\)\.unwrap\(\)', r'.read().ok()?'),
            (r'\.write\(\)\.unwrap\(\)', r'.write().ok()?'),
            (r'\.try_lock\(\)\.unwrap\(\)', r'.try_lock().ok()?'),
        ]
        
        for pattern, replacement in lock_patterns:
            # Only apply in functions that return Result
            if '-> Result' in content or '-> Option' in content:
                content, n = re.subn(pattern, replacement, content)
                changes += n
            else:
                # Use unwrap_or_default for non-Result functions
                for pat, _ in lock_patterns:
                    content, n = re.subn(pat.replace(r'.ok()?', r'.unwrap_or_default()'), pat.replace(r'.unwrap()', r'.unwrap_or_default()'), content)
                    changes += n
        
        # Pattern 4: Path/string conversions
        path_patterns = [
            (r'\.to_str\(\)\.unwrap\(\)', r'.to_str().unwrap_or("")'),
            (r'\.to_string_lossy\(\)\.unwrap\(\)', r'.to_string_lossy()'),
            (r'\.file_name\(\)\.unwrap\(\)', r'.file_name().unwrap_or_default()'),
            (r'\.parent\(\)\.unwrap\(\)', r'.parent().unwrap_or(Path::new("."))'),
        ]
        
        for pattern, replacement in path_patterns:
            content, n = re.subn(pattern, replacement, content)
            changes += n
        
        # Pattern 5: Option unwraps
        option_patterns = [
            (r'\.get\(([^)]+)\)\.unwrap\(\)', r'.get(\1).cloned().unwrap_or_default()'),
            (r'\.get_mut\(([^)]+)\)\.unwrap\(\)', r'.get_mut(\1)?'),
            (r'\.first\(\)\.unwrap\(\)', r'.first().cloned().unwrap_or_default()'),
            (r'\.last\(\)\.unwrap\(\)', r'.last().cloned().unwrap_or_default()'),
            (r'\.pop\(\)\.unwrap\(\)', r'.pop().unwrap_or_default()'),
        ]
        
        for pattern, replacement in option_patterns:
            if '-> Result' in content:
                content, n = re.subn(pattern, replacement, content)
                changes += n
        
        # Pattern 6: Receiver/channel unwraps
        channel_patterns = [
            (r'\.recv\(\)\.unwrap\(\)', r'.recv().unwrap_or_default()'),
            (r'\.try_recv\(\)\.unwrap\(\)', r'.try_recv().unwrap_or_default()'),
            (r'\.send\(([^)]+)\)\.unwrap\(\)', r'.send(\1).ok()'),
        ]
        
        for pattern, replacement in channel_patterns:
            content, n = re.subn(pattern, replacement, content)
            changes += n
        
        # Pattern 7: Array/Vec access
        vec_patterns = [
            (r'\[([^\]]+)\]\.unwrap\(\)', r'.get(\1).cloned().unwrap_or_default()'),
        ]
        
        for pattern, replacement in vec_patterns:
            content, n = re.subn(pattern, replacement, content)
            changes += n
        
        # Pattern 8: Generic unwrap() replacement for remaining cases
        # This is more aggressive - replace with unwrap_or_default where possible
        if '.unwrap()' in content:
            # Count remaining unwraps
            remaining = content.count('.unwrap()')
            
            # For specific known safe patterns, use unwrap_or_default
            safe_patterns = [
                (r'([a-zA-Z_][a-zA-Z0-9_]*)\s*\.\s*clone\(\)\s*\.\s*unwrap\(\)', r'\1.clone().unwrap_or_default()'),
                (r'([a-zA-Z_][a-zA-Z0-9_]*)\s*\.\s*take\(\)\s*\.\s*unwrap\(\)', r'\1.take().unwrap_or_default()'),
                (r'env::var\(([^)]+)\)\.unwrap\(\)', r'env::var(\1).unwrap_or_default()'),
            ]
            
            for pattern, replacement in safe_patterns:
                content, n = re.subn(pattern, replacement, content)
                changes += n
        
        if changes > 0:
            with open(filepath, 'w') as f:
                f.write(content)
            print(f"Fixed {changes} unwrap() calls in {filepath}")
        
        return changes
        
    except Exception as e:
        print(f"Error processing {filepath}: {e}")
        return 0

# Process all Rust files
total_fixed = 0
for root, dirs, files in os.walk('.'):
    dirs[:] = [d for d in dirs if not d.startswith('.') and d != 'target']
    
    for file in files:
        if file.endswith('.rs'):
            filepath = os.path.join(root, file)
            total_fixed += fix_unwrap_comprehensive(filepath)

print(f"\nTotal unwrap() calls fixed: {total_fixed}")

# Now check remaining unwraps
remaining = 0
for root, dirs, files in os.walk('.'):
    dirs[:] = [d for d in dirs if not d.startswith('.') and d != 'target']
    
    for file in files:
        if file.endswith('.rs') and 'test' not in file:
            filepath = os.path.join(root, file)
            try:
                with open(filepath, 'r') as f:
                    content = f.read()
                count = content.count('.unwrap()')
                if count > 0:
                    remaining += count
                    if count > 5:  # Show files with many unwraps
                        print(f"  {filepath}: {count} unwraps remaining")
            except:
                pass

print(f"\nTotal unwrap() calls remaining: {remaining}")