#!/usr/bin/env python3
"""Accurately detect wildcard imports in production code."""

import os
import re

def find_wildcard_imports():
    production_wildcards = []
    
    for root, dirs, files in os.walk('.'):
        # Skip test directories
        dirs[:] = [d for d in dirs if d not in ['tests', 'test', 'examples', 'benches', '.git', 'target']]
        
        for file in files:
            if file.endswith('.rs'):
                filepath = os.path.join(root, file)
                try:
                    with open(filepath, 'r') as f:
                        content = f.read()
                        
                    # Remove all test modules before checking
                    # Match #[cfg(test)] followed by mod tests { /* Implementation needed */ }
                    content = re.sub(r'#\[cfg\(test\)\]\s*mod\s+tests?\s*\{[^{}]*(?:\{[^{}]*\}[^{}]*)*\}', '', content, flags=re.DOTALL)
                    
                    # Now check for wildcard imports
                    lines = content.split('\n')
                    for i, line in enumerate(lines):
                        if re.search(r'use\s+.*::\*;', line) and not line.strip().startswith('//'):
                            production_wildcards.append(f"{filepath}:{i+1}: {line.strip()}")
                            
                except Exception as e:
                    pass
    
    return production_wildcards

if __name__ == '__main__':
    wildcards = find_wildcard_imports()
    
    print(f"Found {len(wildcards)} wildcard imports in production code:")
    for w in wildcards:
        print(w)