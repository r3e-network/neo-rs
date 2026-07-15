# REJECTED: `track_reads_in_write_cache = false` on TX / nested caches

## Change
- `DataCacheConfig.track_reads_in_write_cache` + `clone_cache_with_config`.
- TX child cache and nested `load_script` snapshot clones used
  `track_reads_in_write_cache: false` so cold gets stay parent-backed and skip
  `TrackState::None` materialization into the write dictionary.

## Hypothesis
Fewer write-lock inserts on first get → faster TX load/execute.

## A/B (uncoord dual-DB MPT, h100k→300k, tmpfs)

Official root `@300k` **MATCH** all runs
`0xf0e121ac28b2de37e7e0321b0e0ef926f8b1ff9e85ee6f03c0c6c242a5f31088`.

### vs callflags retained control (~12.28k)
| Metric | Control | Candidate mean | Δ |
|--------|--------:|---------------:|--:|
| Overall blocks/s | 12,284 | **7,598** | **−38%** |
| TX blocks/s | 1,972 | **905** | **−54%** |
| Dense 290–300k | 1,735 | **743** | **−57%** |
| load_execute_us | ~300 | **~745** | **~2.5×** |

### Same-tree control (`track_reads=true` / plain `clone_cache`)
Two-run mean overall **~7,490**, TX **~894**, dense **~736**, load_execute **~785**.
Candidate is **within noise / slightly better** than same-tree control — so the
knob is not the primary 12.3k→7.5k regression, but it also does **not** win on
the retained 12.3k baseline.

## Interpretation
Repeated storage reads in contracts benefit from materializing cold gets into the
child write cache. Skipping that forces repeated parent walks. Net: no keep as a
throughput default.

## Decision
**Reject for throughput retention.** Keep `DataCacheConfig` /
`clone_cache_with_config` infrastructure with **default `track_reads_in_write_cache: true`**.
Default TX/nested paths remain `clone_cache()`.

## Follow-ups (separate from this knob)
This session also restored:
1. `[state_service].coordinated=false` dual-DB MPT path (HEAD had forced coordinated).
2. `post_execute_instruction_enabled` host gate (NoDiagnostic skips per-instruction host callbacks).

After (2), same-tree runs still ~7.5k overall / ~740 dense — **below** the
callflags retained mean (~12.3k / ~1.73k dense). Next session should bisect
what dropped TX `load_execute` from ~300µs to ~740µs on this tree.
