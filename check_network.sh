#\!/bin/bash
cargo check -p neo-network 2>&1 | grep -E "error" -A 3
