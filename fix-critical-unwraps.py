#!/usr/bin/env python3
"""
Neo Rust Critical Unwrap() Migration Script
Identifies and fixes the most critical unwrap() calls in the codebase.
"""

import os
import re
import subprocess
from pathlib import Path

# Critical modules where unwrap() calls are most dangerous
CRITICAL_MODULES = [
    "crates/vm/src/execution_engine.rs",
    "crates/smart_contract/src/application_engine.rs", 
    "crates/network/src/peer_manager.rs",
    "crates/consensus/src/service.rs",
    "crates/core/src/uint160.rs",
    "crates/core/src/uint256.rs"
]

def find_critical_unwraps():
    """Find unwrap() calls in critical execution paths."""
    critical_unwraps = []
    
    for module in CRITICAL_MODULES:
        file_path = f"/home/neo/git/neo-rs/{module}"
        if os.path.exists(file_path):
            with open(file_path, 'r') as f:
                lines = f.readlines()
                for i, line in enumerate(lines, 1):
                    if '.unwrap()' in line and not line.strip().startswith('//'):
                        # Skip test code and comments
                        if '#[test]' not in line and 'expect(' not in line:
                            critical_unwraps.append({
                                'file': module,
                                'line': i,
                                'content': line.strip(),
                                'severity': assess_severity(line)
                            })
    
    return sorted(critical_unwraps, key=lambda x: x['severity'], reverse=True)

def assess_severity(line):
    """Assess the severity of an unwrap() call."""
    severity = 1
    
    # High severity patterns
    if any(pattern in line.lower() for pattern in [
        'pop().unwrap()', 'push().unwrap()', 'execute().unwrap()',
        'validate().unwrap()', 'verify().unwrap()', 'sign().unwrap()'
    ]):
        severity += 3
        
    # Medium severity patterns
    if any(pattern in line.lower() for pattern in [
        'get().unwrap()', 'parse().unwrap()', 'read().unwrap()'
    ]):
        severity += 2
        
    # Low severity patterns  
    if any(pattern in line.lower() for pattern in [
        'clone().unwrap()', 'to_string().unwrap()', 'format().unwrap()'
    ]):
        severity += 1
        
    return severity

def fix_critical_unwrap(file_path, line_num, original_line):
    """Generate a safer alternative to unwrap() call."""
    
    # Common patterns and their replacements
    replacements = {
        r'\.unwrap\(\)': '?',
        r'\.pop\(\)\.unwrap\(\)': '.pop().ok_or(Error::StackUnderflow)?',
        r'\.get\((\w+)\)\.unwrap\(\)': r'.get(\1).ok_or(Error::ItemNotFound)?',
        r'\.parse\(\)\.unwrap\(\)': '.parse().map_err(|_| Error::ParseError)?',
        r'\.clone\(\)\.unwrap\(\)': '.clone()',  # clone() doesn't fail
    }
    
    fixed_line = original_line
    for pattern, replacement in replacements.items():
        fixed_line = re.sub(pattern, replacement, fixed_line)
        
    return fixed_line

def main():
    print("🔧 NEO RUST CRITICAL UNWRAP() MIGRATION TOOL")
    print("=" * 50)
    
    # Find critical unwraps
    unwraps = find_critical_unwraps()
    
    print(f"📊 Found {len(unwraps)} critical unwrap() calls")
    print("\n🎯 TOP 10 MOST CRITICAL:")
    
    for i, unwrap in enumerate(unwraps[:10], 1):
        print(f"{i:2d}. {unwrap['file']}:{unwrap['line']} (severity: {unwrap['severity']})")
        print(f"    {unwrap['content']}")
        print()
        
    print(f"📈 SEVERITY DISTRIBUTION:")
    high = len([u for u in unwraps if u['severity'] >= 4])
    medium = len([u for u in unwraps if 2 <= u['severity'] < 4]) 
    low = len([u for u in unwraps if u['severity'] < 2])
    
    print(f"  🔴 High severity: {high}")
    print(f"  🟡 Medium severity: {medium}")
    print(f"  🟢 Low severity: {low}")
    
    print(f"\n✅ ANALYSIS COMPLETE")
    print(f"💡 Recommendation: Address {high} high-severity unwraps first")

if __name__ == "__main__":
    main()