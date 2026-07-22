# MainNet Continuation 1,300,000-1,400,000

The rebuilt node, with the measured default coordinated budget of `16,384`,
continued the durable full-state replay through height `1,400,000` on
2026-07-16.

- Ledger and StateService heights: `1,400,000`.
- MPT failures: `0`.
- Local root: `0x06062f647755a4550e7dff72039dad5e4f57cf7b2ec99260bea984c70012df7c`.
- `seed1.neo.org:10332 getstateroot(1400000)` returned the same root.
- Overall rate: `180.93` blocks/s.
- Transaction-bearing rate: `721.21` blocks/s (`103,460` transactions).
- Finalization: `492.85 s` of `554.33 s` importer driver time.
- Finalization commit window: `25.93 s` in the final 10,000-block sample;
  the full importer finalization time includes all coordinated windows.
- Empty blocks alone: `41,877.92` blocks/s.

This is a substantially more transaction-dense range than the 1,300,000 to
1,330,000 budget A/B. Native VM execution remained bounded (`44.16 s` total,
about `426 us/transaction`), while durable MDBX/state publication dominated
the wall time. The profile recorded zero deferred-finalization lookup errors,
zero MPT apply failures, and no root drift after reopening.

The raw node log, process timing file, and replay database were intentionally
removed after this bounded summary and its matching StateRoot evidence were
recorded.
