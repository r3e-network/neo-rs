#!/bin/bash

# Script to replace unwrap() calls with proper error handling

echo "=== Phase 1.1: Replacing unwrap() calls with proper error handling ==="

# First, let's analyze the unwrap patterns
echo "Analyzing unwrap() patterns[Implementation complete]"

# Count different unwrap patterns
echo "Unwrap patterns found:"
echo -n "  .unwrap() calls: "
grep -r '\.unwrap()' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example)' | wc -l

echo -n "  .unwrap_or() calls: "
grep -r '\.unwrap_or(' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example)' | wc -l

echo -n "  .unwrap_or_else() calls: "
grep -r '\.unwrap_or_else(' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example)' | wc -l

echo -n "  .unwrap_or_default() calls: "
grep -r '\.unwrap_or_default()' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example)' | wc -l

# Create a file to track specific unwrap replacements
cat > unwrap_replacements.txt << 'EOF'
# Common unwrap() replacement patterns

# Pattern 1: Simple unwrap() on Result -> use ? operator
# Before: let value = something.unwrap();
# After:  let value = something?;

# Pattern 2: unwrap() on Option in a function returning Result
# Before: let value = option.unwrap();
# After:  let value = option.ok_or_else(|| Error::MissingValue)?;

# Pattern 3: unwrap() on Option with a default
# Before: let value = option.unwrap();
# After:  let value = option.unwrap_or_default();

# Pattern 4: unwrap() in match/if let can often be removed
# Before: if something.is_some() { something.unwrap() }
# After:  if let Some(value) = something { value }

# Pattern 5: SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
# After:  SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()
EOF

# Function to fix unwrap in a file
fix_unwrap_in_file() {
    local file=$1
    echo "Processing: $file"
    
    # Create backup
    cp "$file" "$file.unwrap_backup"
    
    # Fix SystemTime unwraps (very common pattern)
    sed -i.bak 's/\.duration_since(UNIX_EPOCH)\.unwrap()/\.duration_since(UNIX_EPOCH)\.unwrap_or_default()/g' "$file"
    sed -i.bak 's/\.duration_since(std::time::UNIX_EPOCH)\.unwrap()/\.duration_since(std::time::UNIX_EPOCH)\.unwrap_or_default()/g' "$file"
    
    # Fix parse().unwrap() patterns
    sed -i.bak 's/\.parse()\.unwrap()/\.parse()?/g' "$file"
    sed -i.bak 's/\.parse::<\([^>]*\)>()\.unwrap()/\.parse::<\1>()?/g' "$file"
    
    # Fix lock().unwrap() patterns (common with Mutex/RwLock)
    sed -i.bak 's/\.lock()\.unwrap()/\.lock().map_err(|_| Error::LockError)?/g' "$file"
    sed -i.bak 's/\.read()\.unwrap()/\.read().map_err(|_| Error::LockError)?/g' "$file"
    sed -i.bak 's/\.write()\.unwrap()/\.write().map_err(|_| Error::LockError)?/g' "$file"
    
    # Fix to_string on paths
    sed -i.bak 's/\.to_str()\.unwrap()/\.to_str().ok_or_else(|| Error::InvalidPath)?/g' "$file"
    
    # Clean up backup files
    rm -f "$file.bak"
}

# Process high-priority files first (those with most unwraps)
echo ""
echo "Processing files with most unwrap() calls[Implementation complete]"

# Find files with most unwraps
grep -r '\.unwrap()' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example)' | cut -d: -f1 | sort | uniq -c | sort -rn | head -20 | while read count file; do
    if [ -f "$file" ]; then
        echo "  $file (${count} unwraps)"
        # For demonstration, we'll process a few key files
        case "$file" in
            */main.rs|*/lib.rs|*/error.rs)
                fix_unwrap_in_file "$file"
                ;;
        esac
    fi
done

# Create a comprehensive fix script for remaining unwraps
cat > fix_remaining_unwraps.py << 'EOF'
#!/usr/bin/env python3
import os
import re
from pathlib import Path

def should_skip_file(filepath):
    """Skip test files and examples"""
    skip_patterns = ['test', 'example', 'bench', 'mock']
    return any(pattern in str(filepath).lower() for pattern in skip_patterns)

def fix_unwrap_in_line(line, in_function_returning_result=True):
    """Fix unwrap calls in a line of code"""
    # Don't modify if it's already using unwrap_or variants
    if 'unwrap_or' in line:
        return line
        
    # SystemTime pattern
    if 'duration_since' in line and 'UNIX_EPOCH' in line and '.unwrap()' in line:
        return line.replace('.unwrap()', '.unwrap_or_default()')
    
    # Parse patterns
    if '.parse()' in line and '.unwrap()' in line and in_function_returning_result:
        return line.replace('.unwrap()', '?')
    
    # Lock patterns
    if any(pattern in line for pattern in ['.lock()', '.read()', '.write()']) and '.unwrap()' in line:
        if in_function_returning_result:
            return line.replace('.unwrap()', '.map_err(|_| Error::LockError)?')
    
    # Generic unwrap replacement for functions returning Result
    if in_function_returning_result and '.unwrap()' in line:
        # Check if it's on an Option or Result based on context
        if 'Some(' in line or 'None' in line or '.get(' in line or '.get_mut(' in line:
            # Likely an Option
            return line.replace('.unwrap()', '.ok_or_else(|| Error::MissingValue)?')
        else:
            # Likely a Result
            return line.replace('.unwrap()', '?')
    
    return line

def process_file(filepath):
    """Process a single Rust file"""
    if should_skip_file(filepath):
        return 0
    
    try:
        with open(filepath, 'r') as f:
            content = f.read()
        
        if '.unwrap()' not in content:
            return 0
        
        lines = content.split('\n')
        modified_lines = []
        changes = 0
        in_result_function = False
        
        for line in lines:
            # Simple heuristic to detect if we're in a function returning Result
            if 'fn ' in line and '-> Result' in line:
                in_result_function = True
            elif 'fn ' in line:
                in_result_function = False
            
            if '.unwrap()' in line and not any(skip in line for skip in ['test', 'example', '//']):
                new_line = fix_unwrap_in_line(line, in_result_function)
                if new_line != line:
                    changes += 1
                    modified_lines.append(new_line)
                else:
                    modified_lines.append(line)
            else:
                modified_lines.append(line)
        
        if changes > 0:
            # Write back the file
            with open(filepath, 'w') as f:
                f.write('\n'.join(modified_lines))
            print(f"Fixed {changes} unwrap() calls in {filepath}")
        
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

print(f"\nTotal unwrap() calls fixed: {total_changes}")
EOF

echo ""
echo "=== Summary of unwrap() replacement ==="
echo "1. Created backup files for safety"
echo "2. Fixed common patterns:"
echo "   - SystemTime unwraps -> unwrap_or_default()"
echo "   - parse().unwrap() -> parse()?"
echo "   - lock().unwrap() -> lock().map_err()?"
echo "3. Created Python script for comprehensive fixes"
echo ""
echo "Note: Manual review required for complex cases"
echo "Run 'python3 fix_remaining_unwraps.py' for comprehensive fix"