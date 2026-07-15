# Recovered: stash@{1} WIP restores ~10k / ~360µs load_execute

## Source
- Base: `c9e9d37e` (docs reject TX-only eager, 17:57)
- Overlay: `git stash apply stash@{1}` (`perf-continue-all-wip-before-mpt-ab`)
- Includes large **neo-vm jump_table / stack_item / evaluation_stack** rewrites
  not present on clean HEAD.

## A/B (uncoord dual-DB, h100k→300k, tmpfs, 2 runs, root MATCH)

| Run | overall | dense | TX | load_execute_us |
|----:|--------:|------:|---:|----------------:|
| 1 | 10,027 | 1,439 | 1,651 | 374 |
| 2 | 10,487 | 1,369 | 1,657 | 361 |
| **mean** | **10,257** | **1,404** | **1,654** | **368** |

Compare: current HEAD ~7.4k / ~748µs; known-fast binary ~11.2k / 318µs.

## Implication
The ~2× pure-execute regression is **uncommitted neo-vm hot-path WIP** left in
`stash@{1}`, not dual-DB config or MPT apply-batch. Next: port neo-vm (+matching
execution) from stash onto HEAD while keeping dual-DB uncoord + warm-cache +
callflags + post_execute gate.

## Ported onto main (same session)

After rsync of stash@{1} neo-vm + StackValue/Interoperable consumers onto
current main (dual-DB uncoord retained), 2-run uncoord h100k→300k:

| Run | overall | dense | TX | load_execute_us | root |
|----:|--------:|------:|---:|----------------:|:----:|
| 1 | **11,653** | **1,597** | 1,829 | **318** | MATCH |
| 2 | **11,414** | **1,447** | 1,862 | **315** | MATCH |
| **mean** | **11,534** | **1,522** | **1,845** | **317** | |

**Historical ~12k / ~300µs band restored.**
