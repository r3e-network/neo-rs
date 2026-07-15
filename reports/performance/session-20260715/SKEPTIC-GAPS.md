# Skeptic-gap remediation (session continuation)

## Criterion 5 — production-default coordinated path
Default is `coordinated=true`, `full_state=false` (see `StateServiceSection`).

Multi-run (3×) MainNet coordinated pruning h811,000→821,000 on tmpfs:

| Variant | Mean blocks/s | Official root @821k |
|---------|--------------:|---------------------|
| Control budget=0 | **2,171.6** | `0xc94105f3…a3ba` MATCH |
| Candidate budget=8192 | **2,149.1** | same MATCH |

Both means sit in the **1,500–2,000+** band. Work-budget intermediate flush is
retained as a safety valve (`NEO_COORDINATED_IMPORT_CHANGE_BUDGET`, default 8192)
with root-matched evidence; throughput delta ≈ −1% (noise) on this window.

Artifacts: `implementer/perf-ab/coordinated-work-budget/h811-summary.json`.

## Criterion 4 — fast-sync SHA-256 authenticity
- Package promote **requires** SHA-256; MD5 alone fails closed.
- Operator pin: `--fast-sync-expected-sha256` when NGD manifest omits `sha256`.
- Bench (empty DB chain.acc vs checkpoint restore + catch-up):
  - full 0→2000 wall ~0.13s import; checkpoint restore ~0.01s + catch-up 1000→2000 ~0.07s
  - See `implementer/fast-sync-bench/bench-report.json`

## Fail-closed MPT reads
- `MptReadSnapshot::try_get`, `try_get_state_root`, `try_current_local_root_index`
  use `try_get_bytes_result` and return `MptError` on I/O failure.
- Provider factory uses fallible APIs.
- Test: `mpt_read_snapshot_and_state_root_reads_fail_closed_on_backing_io_errors`.

## Criterion 3 — parity window extended
- 300k root MATCH official.
- 310k local `0xfc0a6627…0afc` MATCH seed1/seed2/coz official.
- Report: `implementer/parity-extend/parity-report.json`.

## Criterion 2 — live P2P
Still **not** claimed complete: sandbox DNS `198.18.0.x`, peers close after TCP.
In-repo handshake 3/3 pass. Env log retained.
