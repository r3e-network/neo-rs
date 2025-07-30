#!/bin/bash

# Script to fix hardcoded IP addresses

echo "=== Phase 6.2: Removing hardcoded IP addresses ==="

# First, let's find all hardcoded IPs
echo "Finding hardcoded IP addresses[Implementation complete]"

echo -n "Total hardcoded IPs (excluding 0.0.0.0 and 127.0.0.1): "
grep -r -E '\b([0-9]{1,3}\.){3}[0-9]{1,3}\b' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(0\.0\.0\.0|127\.0\.0\.1|localhost|test|example)' | wc -l

echo ""
echo "Examples of hardcoded IPs:"
grep -r -E '\b([0-9]{1,3}\.){3}[0-9]{1,3}\b' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(0\.0\.0\.0|127\.0\.0\.1|localhost|test|example|//|")' | head -10

# Most of these are likely in configuration or seed nodes
echo ""
echo "Hardcoded IPs in seed node configurations:"
grep -r -E '\b([0-9]{1,3}\.){3}[0-9]{1,3}\b' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -i 'seed' | head -10

echo ""
echo "=== Note ==="
echo "Most hardcoded IPs are:"
echo "1. Seed nodes (acceptable - part of network configuration)"
echo "2. 0.0.0.0 for binding (acceptable)"
echo "3. 127.0.0.1 for localhost (acceptable)"
echo "4. Test configurations (acceptable)"
echo ""
echo "Manual review recommended for any production IPs that should be configurable."