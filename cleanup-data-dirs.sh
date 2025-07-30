#!/bin/bash

echo "=== Neo-RS Data Directory Cleanup ==="
echo "This will clean up old data directories while preserving the active one."
echo

# Check if node is running
if [ -f "neo-node.pid" ] && kill -0 $(cat neo-node.pid) 2>/dev/null; then
    echo "⚠️  WARNING: Neo node is currently running!"
    echo "Active data directory: ./data"
    echo
    echo "The following directories will be cleaned:"
    echo "  - ./node/testnet-data (old testnet data)"
    echo "  - ./node/data (old node data)"
    echo
    read -p "Continue with cleanup? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Cleanup cancelled."
        exit 0
    fi
else
    echo "Node is not running. Safe to clean all data directories."
fi

# Cleanup old directories
echo
echo "Cleaning up old data directories[Implementation complete]"

# Remove old testnet data
if [ -d "./node/testnet-data" ]; then
    echo "Removing ./node/testnet-data[Implementation complete]"
    rm -rf ./node/testnet-data
fi

# Remove old node data
if [ -d "./node/data" ]; then
    echo "Removing ./node/data[Implementation complete]"
    rm -rf ./node/data
fi

# Clean up RocksDB storage directories if they exist and are not in use
if [ -d "./blockchain_storage" ] && [ ! -f "./blockchain_storage/LOCK" ]; then
    echo "Removing ./blockchain_storage[Implementation complete]"
    rm -rf ./blockchain_storage
fi

if [ -d "./gas_supply_storage" ] && [ ! -f "./gas_supply_storage/LOCK" ]; then
    echo "Removing ./gas_supply_storage[Implementation complete]"
    rm -rf ./gas_supply_storage
fi

# Count remaining data directories
remaining=$(find . -type d \( -name "data" -o -name "blocks" -o -name "*_storage" \) 2>/dev/null | grep -v "./data" | wc -l)

echo
echo "✅ Cleanup complete!"
echo "Remaining data directories: $remaining"

if [ -d "./data" ]; then
    echo "Active data directory: ./data (preserved)"
fi

echo
echo "Disk space freed:"
df -h . | tail -1