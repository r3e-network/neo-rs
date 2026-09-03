# Transaction Signature Verification Audit

## Status

NEEDS_VERIFICATION

## Tools Created

- `scripts/discover-tx-signature-divergences.py` - Compare Rust vs C# signature verification

## Usage

```bash
./scripts/discover-tx-signature-divergences.py \
  --rust http://localhost:10332 \
  --csharp http://seed1.neo.org:10332 \
  --heights 1,100,1000
```

## Next Steps

1. ✅ Create discovery framework
2. Run against synced node
3. Fix divergences if found
4. Mark VERIFIED_COMPATIBLE if none
