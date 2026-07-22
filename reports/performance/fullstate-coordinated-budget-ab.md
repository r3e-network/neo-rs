# Coordinated Full-State Commit Budget A/B

Captured 2026-07-16 on MainNet heights 811,001-821,000 with deferred full-state
finalization and eight MDBX read workers. Both runs reached the official root
`0xc94105f375f4f38438f129a75d4e172d36f655b82ebe3c78ca2a06179302a3ba` at
height 821,000 with zero MPT failures.

| Projected-change budget | MDBX commits | Overall BPS | Transaction-bearing BPS | Finalization | MDBX total |
|---:|---:|---:|---:|---:|---:|
| 8,192 (default) | 9 | 210.7 | 902.7 | 43.31 s | 24.83 s |
| 0 (single outer batch) | 1 | 123.6 | 952.7 | 76.94 s | 58.88 s |

The single-transaction candidate produced 670,552 MDBX entries and 125 MB of
values in one write transaction. Cursor traversal became nonlinear (48.59 s),
outweighing the saved durability fences. It is rejected for production; the
bounded default remains the safer work budget. The candidate log is
`fullstate-profile-h811-821-budget0-node.log` (SHA-256
`aa2f419eafbb910cbe1cde797f7348b037887d32712a6c6480c8e524766cd56f`).

## Bounded 12,288-Change Candidate

The existing async worker bound of 12,288 projected changes was also tested in
coordinated full-state mode, with eight MDBX readers and the same 10,000-block
window. Both candidate runs reached the official root with zero MPT failures
and used six durable transactions:

| Run | Overall BPS | Transaction-bearing BPS | MDBX total | Cursor writes |
|---|---:|---:|---:|---:|
| 12,288 A | 212.9 | 907.1 | 24.53 s | 5.90 s |
| 12,288 B | 204.2 | 833.0 | 25.45 s | 6.02 s |

The variation overlaps the 8,192-control result (210.7 overall and 902.7
transaction-bearing blocks/s), so the lower commit count does not establish a
throughput win. The production default remains 8,192. Logs:

- `fullstate-profile-h811-821-budget12288-node.log`, SHA-256
  `b139dde491c66930f38a2030829f3f84ed40bb69035b6df4a1bdfcc6ca92b2e6`.
- `fullstate-profile-h811-821-budget12288b-node.log`, SHA-256
  `118b0598ad104e3c766e18da35fb306cc0b5a946a3405f597f8939a4ff196b73`.
