#!/usr/bin/env python3
import os
import re
from pathlib import Path

def fix_println_aggressive(filepath):
    """Aggressively replace all println! with proper logging"""
    if any(skip in str(filepath) for skip in ['target/', '.git/', 'test', 'example']):
        return 0
    
    # Skip CLI files that legitimately need console output
    if 'cli/src/console.rs' in str(filepath) or 'cli/src/main.rs' in str(filepath):
        return 0
    
    try:
        with open(filepath, 'r') as f:
            content = f.read()
        
        if 'println!' not in content:
            return 0
        
        original_content = content
        changes = 0
        
        # Replace different println! patterns
        patterns = [
            # Simple println!
            (r'println!\s*\(\s*\)', 'log::info!("")'),
            
            # println! with string literal
            (r'println!\s*\(\s*"([^"]+)"\s*\)', r'log::info!("\1")'),
            
            # println! with format string and args
            (r'println!\s*\(\s*"([^"]+)"\s*,\s*([^)]+)\s*\)', r'log::info!("\1", \2)'),
            
            # println! with complex expressions
            (r'println!\s*\(\s*\{([^}]+)\}\s*,\s*([^)]+)\s*\)', r'log::info!("{}",\2)'),
        ]
        
        for pattern, replacement in patterns:
            content, n = re.subn(pattern, replacement, content, flags=re.MULTILINE)
            changes += n
        
        # Handle remaining println! statements with a more generic approach
        if 'println!' in content:
            # Count remaining
            remaining = content.count('println!')
            
            # Generic replacement - be careful with this
            lines = content.split('\n')
            new_lines = []
            
            for line in lines:
                if 'println!' in line and '//' not in line:
                    # Determine appropriate log level based on content
                    if any(word in line.lower() for word in ['error', 'fail', 'cannot', 'unable']):
                        new_line = line.replace('println!', 'log::error!')
                    elif any(word in line.lower() for word in ['warn', 'warning', 'caution']):
                        new_line = line.replace('println!', 'log::warn!')
                    elif any(word in line.lower() for word in ['debug', 'trace', 'detail']):
                        new_line = line.replace('println!', 'log::debug!')
                    else:
                        new_line = line.replace('println!', 'log::info!')
                    
                    if new_line != line:
                        changes += 1
                        new_lines.append(new_line)
                    else:
                        new_lines.append(line)
                else:
                    new_lines.append(line)
            
            content = '\n'.join(new_lines)
        
        # Ensure log is imported if we made changes
        if changes > 0 and 'use log::{' not in content and 'use log::' not in content:
            # Find a good place to add the import
            import_added = False
            lines = content.split('\n')
            new_lines = []
            
            for i, line in enumerate(lines):
                new_lines.append(line)
                # Add after other use statements
                if line.startswith('use ') and not import_added:
                    # Check if next line is also a use statement
                    if i + 1 < len(lines) and not lines[i + 1].startswith('use '):
                        new_lines.append('use log::{debug, info, warn, error};')
                        import_added = True
            
            if not import_added:
                # Add at the beginning of the file after any module documentation
                for i, line in enumerate(new_lines):
                    if not line.startswith('//') and line.strip() != '':
                        new_lines.insert(i, 'use log::{debug, info, warn, error};')
                        new_lines.insert(i + 1, '')
                        break
            
            content = '\n'.join(new_lines)
        
        if changes > 0:
            with open(filepath, 'w') as f:
                f.write(content)
            print(f"Fixed {changes} println! statements in {filepath}")
        
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
            total_fixed += fix_println_aggressive(filepath)

print(f"\nTotal println! statements fixed: {total_fixed}")

# Check remaining
remaining = 0
for root, dirs, files in os.walk('.'):
    dirs[:] = [d for d in dirs if not d.startswith('.') and d != 'target']
    
    for file in files:
        if file.endswith('.rs') and 'test' not in file.lower() and 'example' not in file.lower():
            filepath = os.path.join(root, file)
            if 'cli/src/console.rs' in filepath or 'cli/src/main.rs' in filepath:
                continue
            try:
                with open(filepath, 'r') as f:
                    content = f.read()
                count = content.count('println!')
                if count > 0:
                    remaining += count
                    print(f"  {filepath}: {count} println! remaining")
            except:
                pass

print(f"\nTotal println! statements remaining in production code: {remaining}")