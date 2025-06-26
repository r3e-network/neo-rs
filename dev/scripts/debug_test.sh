#!/bin/bash
RUST_LOG=neo_network=debug ./target/debug/neo-node --testnet 2>&1 | grep -A5 -B5 -E "(Sending|header|bytes)" | head -100