# Rejected: Parallel Full Pack Authentication

Date: 2026-07-21 UTC

This experiment measured the authority verifier against the same read-only
MainNet pack and marker on the same host. The pack contained `294` committed
frames, `294` immutable runs, `228,803,968` index entries, and `56,390,498,544`
committed pack bytes. Every run completed with the same mandatory-marker
authority result and the same reachable root.

| Mode | Wall | User CPU | System CPU | Peak RSS |
| --- | ---: | ---: | ---: | ---: |
| Serial frame/run verification (baseline) | `45.34 s` | `27.19 s` | `17.87 s` | `30.8 GiB` |
| Unbounded global Rayon pool | `134.22 s` | `889.16 s` | `20.80 s` | `39.5 GiB` |
| Local Rayon pool capped at 8 | `138.90 s` | `891.99 s` | `19.00 s` | `45.6 GiB` |

The full authority command with explicit `--scrub-indexes` also passed, but its
combined open plus independent scrub cost was `161.86 s`, with `29.9 GiB` peak
RSS and approximately `232 GiB` of filesystem input. These measurements make
the storage/page-cache behavior, rather than serial checksum CPU, the limiting
factor. Parallel verification causes competing large scans and materially
increases wall time and memory pressure.

Decision: reject parallel full-history verification. The production path keeps
the serial fail-closed authentication gate. The next optimization must reduce
bytes read at startup through an authenticated checkpoint/chunk receipt or
lazy, per-chunk verification with a strict read-time fail-closed contract; it
must not trade correctness for an unverified fast open.
