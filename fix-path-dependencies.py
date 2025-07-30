#!/usr/bin/env python3
"""Fix path dependencies in Cargo.toml files for production."""

import os
import re
import glob
import toml

def fix_path_dependencies_in_cargo_toml(file_path):
    """Fix path dependencies in a Cargo.toml file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        # Parse TOML
        try:
            cargo_data = toml.loads(content)
        except:
            print(f"Failed to parse TOML in {file_path}")
            return 0
        
        changes_made = 0
        modified = False
        
        # Check dependencies sections
        for dep_section in ['dependencies', 'dev-dependencies', 'build-dependencies']:
            if dep_section not in cargo_data:
                continue
            
            deps = cargo_data[dep_section]
            for dep_name, dep_value in deps.items():
                if isinstance(dep_value, dict) and 'path' in dep_value:
                    # This is a path dependency
                    if dep_name.startswith('neo-'):
                        # This is an internal neo crate
                        # For production, we should use version instead of path
                        # But for workspace members, path is acceptable
                        
                        # Check if this is a workspace member
                        if 'workspace' in file_path and dep_section == 'dependencies':
                            # Keep path dependencies within workspace
                            continue
                        
                        # For non-workspace or external usage, suggest version
                        print(f"Path dependency found in {file_path}: {dep_name} = {dep_value}")
                        
                        # Add version if not present
                        if 'version' not in dep_value:
                            dep_value['version'] = "0.1.0"
                            modified = True
                            changes_made += 1
        
        # Write back if modified
        if modified:
            with open(file_path, 'w', encoding='utf-8') as f:
                toml.dump(cargo_data, f)
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def create_production_cargo_config():
    """Create a .cargo/config.toml for production builds."""
    cargo_config = """[build]
# Optimize for production
rustflags = ["-C", "opt-level=3", "-C", "lto=true", "-C", "codegen-units=1"]

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
panic = "abort"
strip = true

[profile.production]
inherits = "release"
lto = "fat"
opt-level = 3
codegen-units = 1
"""
    
    os.makedirs('.cargo', exist_ok=True)
    with open('.cargo/config.toml', 'w') as f:
        f.write(cargo_config)
    
    print("Created .cargo/config.toml for production builds")

def main():
    """Main function to fix path dependencies."""
    # Create production cargo config
    create_production_cargo_config()
    
    total_changes = 0
    files_checked = 0
    
    # Find all Cargo.toml files
    for cargo_file in glob.glob('**/Cargo.toml', recursive=True):
        if os.path.isfile(cargo_file):
            files_checked += 1
            changes = fix_path_dependencies_in_cargo_toml(cargo_file)
            total_changes += changes
    
    print(f"\nChecked {files_checked} Cargo.toml files")
    print(f"Total path dependencies with missing versions: {total_changes}")
    
    # Provide recommendations
    print("\nRecommendations for production:")
    print("1. Use workspace dependencies for internal crates")
    print("2. For external usage, publish crates to crates.io")
    print("3. Use version specifiers instead of path dependencies")
    print("4. Consider using [patch] section for temporary overrides")

if __name__ == '__main__':
    main()