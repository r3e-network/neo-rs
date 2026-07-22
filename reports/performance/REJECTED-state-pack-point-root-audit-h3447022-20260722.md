# Rejected State-Pack Point Root Audit at MainNet 3,447,022

## Outcome

The point-lookup implementation of the authoritative checkpoint root-graph
audit was stopped before activation. It had not completed after 13 minutes 26
seconds and had already caused at least 800,057,913,344 bytes of physical
reads. No prepared report was written and the MDBX authority marker remained
the exact 144-byte legacy value.

This is rejected validation evidence, not a block-throughput result. It makes
no blocks-per-second claim.

## Fixed Input

- MainNet height: `3,447,022`
- Checkpoint rows: `228,697,811`
- Checkpoint value bytes: `47,682,080,563`
- Pack payload bytes: `60,489,232,219`
- Live index runs: `13`
- Pack segments: `15`
- Checkpoint SHA-256:
  `89757fcec114f96e35e4fa7f5c135f8ccf8f57a6e1b137ea85d0c605020ef43f`
- Activation binary SHA-256:
  `5773aab99ef27b8ef689c26263206b1215cde1be1db955fcd409f757639bb012`

The run used an independent MDBX copy and a pack clone with a physically
independent writable tip segment. Sealed segments and immutable index runs
were hard-linked to avoid duplicating roughly 70 GiB of immutable evidence.

## Profile

`pidstat -dru -p <pid> 2 3` during the root traversal reported:

- CPU: `95.67%` average on one core
- User CPU: `2.00%`
- System CPU: `93.67%`
- Read rate: `1,310,364.67 KiB/s`
- RSS: approximately `15.4 GiB`
- Major faults: `305.67/s`

`/proc/<pid>/io` at 13 minutes 26 seconds reported
`800,057,913,344` physical read bytes. The process mapped all 15 frame
segments and 13 live index runs. `perf` hardware counters were unavailable
because the host has `kernel.perf_event_paranoid=4`.

## Diagnosis

`validate_persisted_root_graph` performs one `Snapshot::get_bounded` point
lookup per reachable content hash. Each lookup probes the large immutable
sorted runs independently. The root graph therefore turns a bounded logical
read into repeated sparse index and payload page faults; MPT decoding is not
the dominant cost.

The replacement must preserve the canonical node decoder, hash binding,
cycle detection, distinct-node accounting, and all node/byte ceilings while
issuing sorted bounded batches against the pinned pack snapshot. The same
clone will be reused for the candidate measurement.

## Abort Safety

After interruption:

- the activation report, prepared report, and staging report were absent;
- the clone authority marker still had SHA-256
  `51a332194661559c0831ce90db18d56c8f562f3d478f1a28f3bb95bb6a86f4f3`;
- the cloned writable tip segment still matched the source SHA-256
  `3e6587963bd53a3c3685b8cb6a594123f105d7f05bb653d8d6646663c6d2dac0`.
