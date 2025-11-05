# Scripts

- `test_all.sh`: runs `cargo test` for each standalone crate in sequence,
  including the `neo-base` derive feature and the integration demo.
- `check_port_parity.py`: helper used by other repo tooling to ensure parity
  between port assignments (kept for compatibility).

Example:

```bash
./scripts/test_all.sh
```
