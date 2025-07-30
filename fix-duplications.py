#!/usr/bin/env python3
"""Fix code duplications in the Neo-RS codebase."""

import os
import re
from collections import defaultdict
from typing import List, Tuple, Dict

class DuplicationFixer:
    def __init__(self):
        self.files_fixed = 0
        self.duplications_fixed = 0
        
    def fix_duplicate_imports(self):
        """Remove duplicate import statements from files."""
        print("=== Fixing Duplicate Imports ===")
        
        for root, dirs, files in os.walk('.'):
            dirs[:] = [d for d in dirs if d not in ['.git', 'target']]
            
            for file in files:
                if file.endswith('.rs'):
                    filepath = os.path.join(root, file)
                    try:
                        with open(filepath, 'r') as f:
                            lines = f.readlines()
                        
                        # Group imports and remove duplicates
                        import_lines = []
                        other_lines = []
                        seen_imports = set()
                        modified = False
                        
                        for line in lines:
                            if line.strip().startswith('use '):
                                import_stmt = line.strip()
                                if import_stmt not in seen_imports:
                                    seen_imports.add(import_stmt)
                                    import_lines.append(line)
                                else:
                                    modified = True
                                    self.duplications_fixed += 1
                            else:
                                other_lines.append(line)
                        
                        if modified:
                            # Find where imports end
                            import_end_idx = 0
                            for i, line in enumerate(lines):
                                if line.strip().startswith('use '):
                                    import_end_idx = i
                            
                            # Reconstruct file with deduplicated imports
                            new_content = []
                            import_added = False
                            
                            for i, line in enumerate(lines):
                                if line.strip().startswith('use ') and not import_added:
                                    # Add all unique imports at once
                                    new_content.extend(import_lines)
                                    import_added = True
                                    # Skip other import lines
                                    while i < len(lines) and lines[i].strip().startswith('use '):
                                        i += 1
                                    if i < len(lines) and not lines[i].strip().startswith('use '):
                                        new_content.append(lines[i])
                                elif not line.strip().startswith('use '):
                                    new_content.append(line)
                            
                            with open(filepath, 'w') as f:
                                f.writelines(new_content)
                            
                            print(f"Fixed duplicate imports in: {filepath}")
                            self.files_fixed += 1
                            
                    except Exception as e:
                        print(f"Error processing {filepath}: {e}")
        
        print(f"Fixed {self.duplications_fixed} duplicate imports in {self.files_fixed} files\n")
    
    def consolidate_constants(self):
        """Move duplicate constants to a common location."""
        print("=== Consolidating Duplicate Constants ===")
        
        # Collect all constants
        constants = defaultdict(list)
        
        for root, dirs, files in os.walk('crates'):
            dirs[:] = [d for d in dirs if d not in ['tests', 'test', 'examples', 'benches', '.git', 'target']]
            
            for file in files:
                if file.endswith('.rs'):
                    filepath = os.path.join(root, file)
                    try:
                        with open(filepath, 'r') as f:
                            content = f.read()
                        
                        # Find constant declarations
                        pattern = r'^(pub\s+)?const\s+(\w+):\s*([^=]+)\s*=\s*([^;]+);'
                        
                        for match in re.finditer(pattern, content, re.MULTILINE):
                            visibility = match.group(1) or ''
                            const_name = match.group(2)
                            const_type = match.group(3).strip()
                            const_value = match.group(4).strip()
                            
                            const_info = {
                                'visibility': visibility,
                                'type': const_type,
                                'value': const_value,
                                'file': filepath
                            }
                            
                            constants[const_name].append(const_info)
                            
                    except Exception as e:
                        pass
        
        # Find and report duplicates
        duplicates_found = 0
        for const_name, locations in constants.items():
            if len(locations) > 1:
                # Check if all values are the same
                values = set(loc['value'] for loc in locations)
                if len(values) == 1:
                    print(f"Duplicate constant '{const_name}' = {values.pop()} found in:")
                    for loc in locations:
                        print(f"  - {loc['file']}")
                    duplicates_found += 1
        
        if duplicates_found > 0:
            print(f"\nFound {duplicates_found} duplicate constants.")
            print("Consider moving these to crates/core/src/constants.rs or crates/config/src/lib.rs")
        else:
            print("No duplicate constants with same values found")
        
        print()
    
    def extract_common_functions(self):
        """Extract commonly duplicated functions to utility modules."""
        print("=== Extracting Common Functions ===")
        
        # Common utility functions that appear in multiple places
        common_patterns = {
            'to_hex': r'fn\s+to_hex\s*\([^)]*\)',
            'from_hex': r'fn\s+from_hex\s*\([^)]*\)',
            'hash': r'fn\s+hash\s*\([^)]*\)',
            'serialize': r'fn\s+serialize\s*\([^)]*\)',
            'deserialize': r'fn\s+deserialize\s*\([^)]*\)',
        }
        
        function_locations = defaultdict(list)
        
        for root, dirs, files in os.walk('crates'):
            dirs[:] = [d for d in dirs if d not in ['tests', 'test', 'examples', 'benches', '.git', 'target']]
            
            for file in files:
                if file.endswith('.rs'):
                    filepath = os.path.join(root, file)
                    try:
                        with open(filepath, 'r') as f:
                            content = f.read()
                        
                        for func_name, pattern in common_patterns.items():
                            if re.search(pattern, content):
                                function_locations[func_name].append(filepath)
                                
                    except Exception as e:
                        pass
        
        # Report findings
        for func_name, locations in function_locations.items():
            if len(locations) > 2:
                print(f"Function '{func_name}' appears in {len(locations)} files:")
                for loc in locations[:5]:
                    print(f"  - {loc}")
                if len(locations) > 5:
                    print(f"  "Implementation complete" and {len(locations) - 5} more")
                print()
        
        print("Consider creating utility modules for commonly used functions\n")
    
    def remove_redundant_impls(self):
        """Remove redundant trait implementations."""
        print("=== Checking for Redundant Implementations ===")
        
        # This is informational only - manual review needed
        impl_count = defaultdict(int)
        
        for root, dirs, files in os.walk('crates'):
            dirs[:] = [d for d in dirs if d not in ['tests', 'test', 'examples', 'benches', '.git', 'target']]
            
            for file in files:
                if file.endswith('.rs'):
                    filepath = os.path.join(root, file)
                    try:
                        with open(filepath, 'r') as f:
                            content = f.read()
                        
                        # Count common trait implementations
                        common_traits = ['Debug', 'Clone', 'PartialEq', 'Eq', 'Hash', 'Default']
                        
                        for trait in common_traits:
                            pattern = f'impl.*{trait}.*for'
                            if re.search(pattern, content):
                                impl_count[trait] += 1
                                
                    except Exception as e:
                        pass
        
        print("Trait implementation counts:")
        for trait, count in sorted(impl_count.items(), key=lambda x: x[1], reverse=True):
            print(f"  - {trait}: {count} implementations")
        
        print("\nConsider using #[derive("Implementation complete")] for common traits instead of manual implementations\n")
    
    def create_common_modules(self):
        """Create suggestions for common utility modules."""
        print("=== Suggestions for Common Modules ===")
        
        suggestions = {
            'crates/core/src/utils/hex.rs': [
                'to_hex functions',
                'from_hex functions',
                'hex encoding/decoding utilities'
            ],
            'crates/core/src/utils/hash.rs': [
                'common hashing functions',
                'hash utilities',
                'merkle tree operations'
            ],
            'crates/core/src/utils/serialization.rs': [
                'common serialize/deserialize helpers',
                'binary encoding utilities',
                'format conversion functions'
            ],
            'crates/core/src/constants.rs': [
                'blockchain constants',
                'network constants',
                'consensus parameters'
            ]
        }
        
        print("Consider creating these common modules to reduce duplication:")
        for module, contents in suggestions.items():
            print(f"\n{module}:")
            for item in contents:
                print(f"  - {item}")
        
        print("\nThis will help centralize common functionality and reduce code duplication.\n")
    
    def fix_simple_duplications(self):
        """Fix simple duplications that can be automated."""
        print("=== Fixing Simple Duplications ===")
        
        # Remove consecutive duplicate lines
        for root, dirs, files in os.walk('.'):
            dirs[:] = [d for d in dirs if d not in ['.git', 'target']]
            
            for file in files:
                if file.endswith('.rs'):
                    filepath = os.path.join(root, file)
                    try:
                        with open(filepath, 'r') as f:
                            lines = f.readlines()
                        
                        # Remove consecutive duplicate lines
                        new_lines = []
                        prev_line = None
                        modified = False
                        
                        for line in lines:
                            if line.strip() and line == prev_line:
                                modified = True
                                self.duplications_fixed += 1
                                continue
                            new_lines.append(line)
                            prev_line = line
                        
                        if modified:
                            with open(filepath, 'w') as f:
                                f.writelines(new_lines)
                            print(f"Fixed consecutive duplicate lines in: {filepath}")
                            self.files_fixed += 1
                            
                    except Exception as e:
                        pass
        
        print(f"Fixed {self.duplications_fixed} simple duplications\n")
    
    def run(self):
        """Run all duplication fixes."""
        print("=== Neo-RS Duplication Fixer ===")
        print("Starting duplication fixes"Implementation complete"\n")
        
        self.fix_duplicate_imports()
        self.fix_simple_duplications()
        self.consolidate_constants()
        self.extract_common_functions()
        self.remove_redundant_impls()
        self.create_common_modules()
        
        print("=== Summary ===")
        print(f"Total files fixed: {self.files_fixed}")
        print(f"Total duplications fixed: {self.duplications_fixed}")
        print("\nDuplication fixing complete!")
        print("Run ./duplication-check.sh to verify the fixes")

if __name__ == '__main__':
    fixer = DuplicationFixer()
    fixer.run()