#!/usr/bin/env python3
"""
Compare transaction fee calculation between neo-rs and C# using mainnet data.
Tests CRITICAL-004 with real transactions.
"""
import json
import urllib.request

# Real mainnet transactions for testing
TEST_TRANSACTIONS = [
    "0x6c12841f2477e13b375ef22ec9bfcc5288ed68b0d1b5fc97d4c6c3a7bcf7b90d",  # Block 38781
    "0x21b17473c89da950f34ff38dc6a305a0ec3c054974797ed722edfa59bf5643be",  # Block 38791
]

def rpc_call(url, method, params):
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    req = urllib.request.Request(url, data=payload.encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=20) as resp:
        return json.loads(resp.read())["result"]

def get_tx_data(rpc_url, txid):
    """Fetch transaction data from RPC"""
    tx = rpc_call(rpc_url, "getrawtransaction", [txid, 1])
    return {
        "hash": tx["hash"],
        "size": tx["size"],
        "netfee": int(tx["netfee"]),
        "sysfee": int(tx["sysfee"]),
        "witnesses": len(tx["witnesses"]),
    }

def main():
    rust_rpc = "http://localhost:10332"
    csharp_rpc = "http://seed1.neo.org:10332"  # Public C# mainnet node
    
    print("Comparing fee calculation: neo-rs vs C#\n")
    
    for txid in TEST_TRANSACTIONS:
        print(f"Transaction: {txid}")
        
        try:
            rust_data = get_tx_data(rust_rpc, txid)
            csharp_data = get_tx_data(csharp_rpc, txid)
            
            if rust_data["netfee"] == csharp_data["netfee"]:
                print(f"  ✓ Network fee matches: {rust_data['netfee']}")
            else:
                print(f"  ✗ DIVERGENCE: Rust={rust_data['netfee']}, C#={csharp_data['netfee']}")
                
        except Exception as e:
            print(f"  Error: {e}")
        print()

if __name__ == "__main__":
    main()
