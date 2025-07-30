#!/bin/bash

echo "=== Comprehensive unwrap() fixing ==="

# Fix lock().unwrap() patterns
echo "Fixing lock().unwrap() patterns[Implementation complete]"
find crates -name "*.rs" -type f | while read file; do
    if [[ "$file" =~ "test" ]]; then continue; fi
    
    # Fix RwLock/Mutex lock unwraps
    sed -i '' 's/\.lock()\.unwrap()/\.lock()\.map_err(|_| anyhow!("Failed to acquire lock"))?/g' "$file"
    sed -i '' 's/\.read()\.unwrap()/\.read()\.map_err(|_| anyhow!("Failed to acquire read lock"))?/g' "$file"
    sed -i '' 's/\.write()\.unwrap()/\.write()\.map_err(|_| anyhow!("Failed to acquire write lock"))?/g' "$file"
done

# Fix parse().unwrap() patterns
echo "Fixing parse().unwrap() patterns[Implementation complete]"
find crates -name "*.rs" -type f | while read file; do
    if [[ "$file" =~ "test" ]]; then continue; fi
    
    sed -i '' 's/\.parse()\.unwrap()/\.parse()?/g' "$file"
    sed -i '' 's/\.parse::<\([^>]*\)>()\.unwrap()/\.parse::<\1>()?/g' "$file"
done

# Fix from_str().unwrap() patterns
echo "Fixing from_str().unwrap() patterns[Implementation complete]"
find crates -name "*.rs" -type f | while read file; do
    if [[ "$file" =~ "test" ]]; then continue; fi
    
    sed -i '' 's/::from_str([^)]*)\.unwrap()/::from_str(\1)?/g' "$file"
done

# Fix Option unwraps that should use ok_or
echo "Fixing Option unwraps[Implementation complete]"
find crates -name "*.rs" -type f | while read file; do
    if [[ "$file" =~ "test" ]]; then continue; fi
    
    # Fix get().unwrap() patterns
    sed -i '' 's/\.get(\([^)]*\))\.unwrap()/\.get(\1).ok_or_else(|| anyhow!("Key not found"))?/g' "$file"
    sed -i '' 's/\.get_mut(\([^)]*\))\.unwrap()/\.get_mut(\1).ok_or_else(|| anyhow!("Key not found"))?/g' "$file"
    
    # Fix first/last unwraps
    sed -i '' 's/\.first()\.unwrap()/\.first().ok_or_else(|| anyhow!("Empty collection"))?/g' "$file"
    sed -i '' 's/\.last()\.unwrap()/\.last().ok_or_else(|| anyhow!("Empty collection"))?/g' "$file"
done

# Count remaining unwraps
echo "Counting remaining unwraps[Implementation complete]"
REMAINING=$(grep -r "\.unwrap()" crates --include="*.rs" | grep -v "test" | wc -l)
echo "Remaining unwrap() calls: $REMAINING"