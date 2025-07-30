#!/bin/bash

# Script to replace panic! statements with proper error handling

echo "=== Phase 1.2: Replacing panic! statements with proper error handling ==="

# First, let's analyze panic patterns
echo "Analyzing panic! patterns[Implementation complete]"

# Count different panic patterns
echo "Panic patterns found:"
echo -n "  panic!() calls: "
grep -r 'panic!' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|unreachable!|unimplemented!)' | wc -l

echo -n "  In production code (excluding tests): "
grep -r 'panic!' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example|#\[cfg\(test\)\])' | grep -v -E '(unreachable!|unimplemented!)' | wc -l

# Show some examples
echo ""
echo "Examples of panic! usage:"
grep -r 'panic!' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|unreachable!|unimplemented!)' | head -5

# Create replacement script
cat > fix_panic_in_file.py << 'EOF'
#!/usr/bin/env python3
import os
import re
from pathlib import Path

def should_skip_file(filepath):
    """Skip test files and examples"""
    skip_patterns = ['test', 'example', 'bench', 'mock']
    return any(pattern in str(filepath).lower() for pattern in skip_patterns)

def fix_panic_in_line(line, context_lines_before=[], context_lines_after=[]):
    """Fix panic! calls in a line of code"""
    # Don't modify unreachable! or unimplemented!
    if 'unreachable!' in line or 'unimplemented!' in line:
        return line
    
    # Don't modify if in test context
    if any('#[test]' in ctx or '#[cfg(test)]' in ctx for ctx in context_lines_before[-5:]):
        return line
    
    # Pattern: panic!("message") -> return Err(Error::new("message"))
    if 'panic!(' in line:
        # Extract the panic message
        match = re.search(r'panic!\s*\(\s*"([^"]+)"\s*\)', line)
        if match:
            message = match.group(1)
            indent = len(line) - len(line.lstrip())
            # Replace with error return
            new_line = ' ' * indent + f'return Err(Error::Other("{message}".to_string()));'
            return new_line
        
        # Pattern: panic!("message {}", var) -> return Err(Error::new(format!([Implementation complete])))
        match = re.search(r'panic!\s*\(\s*"([^"]+)"\s*,\s*(.+)\s*\)', line)
        if match:
            message = match.group(1)
            args = match.group(2)
            indent = len(line) - len(line.lstrip())
            new_line = ' ' * indent + f'return Err(Error::Other(format!("{message}", {args})));'
            return new_line
        
        # Generic panic! without message
        if 'panic!()' in line:
            indent = len(line) - len(line.lstrip())
            new_line = ' ' * indent + 'return Err(Error::InternalError);'
            return new_line
    
    return line

def process_file(filepath):
    """Process a single Rust file"""
    if should_skip_file(filepath):
        return 0
    
    try:
        with open(filepath, 'r') as f:
            content = f.read()
        
        if 'panic!' not in content:
            return 0
        
        lines = content.split('\n')
        modified_lines = []
        changes = 0
        
        for i, line in enumerate(lines):
            context_before = lines[max(0, i-5):i]
            context_after = lines[i+1:min(len(lines), i+6)]
            
            if 'panic!' in line and not any(skip in line for skip in ['unreachable!', 'unimplemented!', '//']):
                new_line = fix_panic_in_line(line, context_before, context_after)
                if new_line != line:
                    changes += 1
                    modified_lines.append(new_line)
                    print(f"  Fixed panic! in {filepath}:{i+1}")
                else:
                    modified_lines.append(line)
            else:
                modified_lines.append(line)
        
        if changes > 0:
            # Write back the file
            with open(filepath, 'w') as f:
                f.write('\n'.join(modified_lines))
            print(f"Fixed {changes} panic! calls in {filepath}")
        
        return changes
    except Exception as e:
        print(f"Error processing {filepath}: {e}")
        return 0

# Main execution
total_changes = 0
for root, dirs, files in os.walk('.'):
    # Skip hidden directories and target
    dirs[:] = [d for d in dirs if not d.startswith('.') and d != 'target']
    
    for file in files:
        if file.endswith('.rs'):
            filepath = os.path.join(root, file)
            total_changes += process_file(filepath)

print(f"\nTotal panic! calls fixed: {total_changes}")
EOF

# Make script executable
chmod +x fix_panic_in_file.py

# Run the Python script
echo ""
echo "Running panic! fix script[Implementation complete]"
python3 fix_panic_in_file.py

echo ""
echo "=== Summary of panic! replacement ==="
echo "1. Replaced panic! with proper error returns"
echo "2. Preserved unreachable! and unimplemented! macros"
echo "3. Skipped test files"
echo ""
echo "Note: Manual review required for complex cases"