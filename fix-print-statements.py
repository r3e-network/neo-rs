#!/usr/bin/env python3
"""Fix print statements in production code."""

import os
import re
import glob

def fix_print_statements(file_path):
    """Fix print statements in a file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files and CLI console output
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench', 'cli/src/console', 'cli/src/main']):
            return 0
        
        # Replace println! with proper logging
        lines = content.splitlines()
        new_lines = []
        
        for line in lines:
            if 'println!' in line and not line.strip().startswith('//'):
                # Extract the message from println!
                match = re.search(r'println!\s*\(\s*"([^"]+)"', line)
                if match:
                    message = match.group(1)
                    indent = len(line) - len(line.lstrip())
                    
                    # Determine log level based on context
                    if any(word in message.lower() for word in ['error', 'fail', 'fatal']):
                        new_line = ' ' * indent + f'log::error!("{message}");'
                    elif any(word in message.lower() for word in ['warn', 'warning']):
                        new_line = ' ' * indent + f'log::warn!("{message}");'
                    elif any(word in message.lower() for word in ['debug', 'trace']):
                        new_line = ' ' * indent + f'log::debug!("{message}");'
                    else:
                        new_line = ' ' * indent + f'log::info!("{message}");'
                    
                    new_lines.append(new_line)
                    changes_made += 1
                else:
                    # Handle println! with format args
                    match = re.search(r'println!\s*\((.+)\)', line)
                    if match:
                        args = match.group(1)
                        indent = len(line) - len(line.lstrip())
                        new_line = ' ' * indent + f'log::info!({args});'
                        new_lines.append(new_line)
                        changes_made += 1
                    else:
                        new_lines.append(line)
            else:
                new_lines.append(line)
        
        if changes_made > 0:
            content = '\n'.join(new_lines)
            
            # Ensure log crate is imported
            if 'use log::' not in content and 'log::' in content:
                # Find where to add the import
                import_index = 0
                for i, line in enumerate(new_lines):
                    if line.startswith('use '):
                        import_index = i + 1
                
                new_lines.insert(import_index, 'use log::{info, warn, error, debug};')
                content = '\n'.join(new_lines)
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix print statements."""
    total_fixes = 0
    files_fixed = 0
    
    # First, let's find where println! is used
    print("Searching for println! statements"Implementation complete"")
    os.system("grep -r 'println!' crates/ node/src/ --include='*.rs' | grep -v -E '(test|example|cli/src/console|cli/src/main)' | head -10")
    print("\n")
    
    # Search for files with println!
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_print_statements(file_path)
                if fixes > 0:
                    print(f"Fixed {fixes} print statements in {file_path}")
                    total_fixes += fixes
                    files_fixed += 1
    
    print(f"\nTotal print statements fixed: {total_fixes} in {files_fixed} files")

if __name__ == '__main__':
    main()