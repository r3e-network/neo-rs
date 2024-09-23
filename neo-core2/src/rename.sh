#!/bin/bash

# Function to create mod.rs file
create_mod_rs() {
    local dir=$1
    local mod_file="$dir/mod.rs"
    
    # Create mod.rs if it doesn't exist
    if [ ! -f "$mod_file" ]; then
        touch "$mod_file"
    fi
    
    # Clear the file
    > "$mod_file"
    
    # Add module declarations
    for item in "$dir"/*; do
        if [ -d "$item" ]; then
            module_name=$(basename "$item")
            echo "pub mod $module_name;" >> "$mod_file"
        elif [ -f "$item" ] && [ "$(basename "$item")" != "mod.rs" ] && [ "$(basename "$item")" != "lib.rs" ]; then
            module_name=$(basename "$item" .rs)
            echo "pub mod $module_name;" >> "$mod_file"
        fi
    done
    
    # Add pub use statements
    echo "" >> "$mod_file"
    for item in "$dir"/*; do
        if [ -d "$item" ]; then
            module_name=$(basename "$item")
            echo "pub use $module_name::*;" >> "$mod_file"
        elif [ -f "$item" ] && [ "$(basename "$item")" != "mod.rs" ] && [ "$(basename "$item")" != "lib.rs" ]; then
            module_name=$(basename "$item" .rs)
            echo "pub use $module_name::*;" >> "$mod_file"
        fi
    done
}

# Function to create lib.rs file
create_lib_rs() {
    local root_dir=$1
    local lib_file="$root_dir/lib.rs"
    
    # Create lib.rs if it doesn't exist
    if [ ! -f "$lib_file" ]; then
        touch "$lib_file"
    fi
    
    # Clear the file
    > "$lib_file"
    
    # Add module declarations
    for item in "$root_dir"/*; do
        if [ -d "$item" ]; then
            module_name=$(basename "$item")
            echo "pub mod $module_name;" >> "$lib_file"
        elif [ -f "$item" ] && [ "$(basename "$item")" != "mod.rs" ] && [ "$(basename "$item")" != "lib.rs" ]; then
            module_name=$(basename "$item" .rs)
            echo "pub mod $module_name;" >> "$lib_file"
        fi
    done
}

# Main script
main_dir=$(pwd)

# Create lib.rs in the root directory
create_lib_rs "$main_dir"

# Create mod.rs for each subdirectory
find "$main_dir" -type d | while read dir; do
    if [ "$dir" != "$main_dir" ]; then
        create_mod_rs "$dir"
    fi
done

echo "Module structure has been generated successfully."