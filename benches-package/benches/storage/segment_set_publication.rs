//! Diagnostic component benchmark for immutable segment-view publication.
//!
//! This deliberately has no dependency on `neo-state-packs`, so it can compare
//! ownership shapes without mixing mmap creation, file I/O, or store recovery
//! into the measured interval. It is not node throughput evidence.

use std::{
    env,
    hint::black_box,
    sync::Arc,
    time::{Duration, Instant},
};

const DEFAULT_TRIALS: usize = 7;
const DEFAULT_TARGET_CLONE_SLOTS: usize = 2_000_000;
const DEFAULT_MIN_ITERATIONS: usize = 1_000;
const DEFAULT_MAX_ITERATIONS: usize = 2_000_000;
const SEGMENT_COUNTS: &[usize] = &[1, 4, 16, 64, 256, 1_024, 4_096];

#[derive(Debug)]
struct Mapping {
    segment_id: usize,
    generation: usize,
}

#[derive(Debug)]
struct CloneAllView {
    mappings: Box<[Arc<Mapping>]>,
}

impl CloneAllView {
    #[inline(never)]
    fn replace_tip(&self, tip: Arc<Mapping>) -> Self {
        let mut mappings = self.mappings.to_vec();
        let last = mappings
            .last_mut()
            .expect("the benchmark always constructs a non-empty view");
        assert_eq!(last.segment_id, tip.segment_id);
        *last = tip;
        Self {
            mappings: mappings.into_boxed_slice(),
        }
    }

    fn mapping(&self, segment_id: usize) -> &Mapping {
        &self.mappings[segment_id]
    }
}

#[derive(Debug)]
struct SharedPrefixView {
    sealed: Arc<[Arc<Mapping>]>,
    tip: Arc<Mapping>,
}

impl SharedPrefixView {
    #[inline(never)]
    fn replace_tip(&self, tip: Arc<Mapping>) -> Self {
        assert_eq!(self.tip.segment_id, tip.segment_id);
        Self {
            sealed: Arc::clone(&self.sealed),
            tip,
        }
    }

    fn mapping(&self, segment_id: usize) -> &Mapping {
        if segment_id == self.sealed.len() {
            &self.tip
        } else {
            &self.sealed[segment_id]
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Config {
    trials: usize,
    target_clone_slots: usize,
    min_iterations: usize,
    max_iterations: usize,
}

impl Config {
    fn from_env() -> Self {
        Self {
            trials: env_usize("NEO_SEGMENT_SET_BENCH_TRIALS", DEFAULT_TRIALS),
            target_clone_slots: env_usize(
                "NEO_SEGMENT_SET_BENCH_TARGET_CLONE_SLOTS",
                DEFAULT_TARGET_CLONE_SLOTS,
            ),
            min_iterations: env_usize(
                "NEO_SEGMENT_SET_BENCH_MIN_ITERATIONS",
                DEFAULT_MIN_ITERATIONS,
            ),
            max_iterations: env_usize(
                "NEO_SEGMENT_SET_BENCH_MAX_ITERATIONS",
                DEFAULT_MAX_ITERATIONS,
            ),
        }
    }

    fn validate(self) -> Self {
        assert!(self.trials > 0, "trial count must be non-zero");
        assert!(
            self.min_iterations > 0,
            "minimum iteration count must be non-zero"
        );
        assert!(
            self.min_iterations <= self.max_iterations,
            "minimum iteration count exceeds maximum"
        );
        self
    }

    fn iterations(self, segment_count: usize) -> usize {
        self.target_clone_slots
            .checked_div(segment_count)
            .unwrap_or_default()
            .clamp(self.min_iterations, self.max_iterations)
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name).map_or(default, |value| {
        value
            .parse()
            .unwrap_or_else(|error| panic!("invalid {name}={value:?}: {error}"))
    })
}

fn initial_mappings(segment_count: usize) -> Vec<Arc<Mapping>> {
    (0..segment_count)
        .map(|segment_id| {
            Arc::new(Mapping {
                segment_id,
                generation: 0,
            })
        })
        .collect()
}

fn replacement_tips(segment_count: usize) -> [Arc<Mapping>; 2] {
    let segment_id = segment_count - 1;
    [
        Arc::new(Mapping {
            segment_id,
            generation: 1,
        }),
        Arc::new(Mapping {
            segment_id,
            generation: 2,
        }),
    ]
}

fn measure_clone_all(
    initial: Arc<CloneAllView>,
    tips: &[Arc<Mapping>; 2],
    iterations: usize,
) -> (Duration, u64) {
    let mut current = initial;
    let started = Instant::now();
    for iteration in 0..iterations {
        current = Arc::new(current.replace_tip(Arc::clone(&tips[iteration & 1])));
        black_box(&current);
    }
    let elapsed = started.elapsed();
    (elapsed, clone_all_fingerprint(&current))
}

fn measure_shared_prefix(
    initial: Arc<SharedPrefixView>,
    tips: &[Arc<Mapping>; 2],
    iterations: usize,
) -> (Duration, u64) {
    let mut current = initial;
    let started = Instant::now();
    for iteration in 0..iterations {
        current = Arc::new(current.replace_tip(Arc::clone(&tips[iteration & 1])));
        black_box(&current);
    }
    let elapsed = started.elapsed();
    (elapsed, shared_prefix_fingerprint(&current))
}

fn clone_all_fingerprint(view: &CloneAllView) -> u64 {
    mapping_fingerprint(view.mappings.len(), |id| view.mapping(id))
}

fn shared_prefix_fingerprint(view: &SharedPrefixView) -> u64 {
    mapping_fingerprint(view.sealed.len() + 1, |id| view.mapping(id))
}

fn mapping_fingerprint<'a>(segment_count: usize, mapping: impl Fn(usize) -> &'a Mapping) -> u64 {
    [0, segment_count / 2, segment_count - 1].into_iter().fold(
        segment_count as u64,
        |checksum, segment_id| {
            let mapping = mapping(segment_id);
            checksum
                .wrapping_mul(1_099_511_628_211)
                .wrapping_add(mapping.segment_id as u64)
                .wrapping_add((mapping.generation as u64) << 32)
        },
    )
}

fn median_ns_per_publication(mut samples: Vec<Duration>, iterations: usize) -> f64 {
    samples.sort_unstable();
    samples[samples.len() / 2].as_secs_f64() * 1_000_000_000.0 / iterations as f64
}

fn main() {
    let config = Config::from_env().validate();
    println!("status=no throughput evidence");
    println!("scope=component successor-view publication only; no node BPS established");
    println!(
        "config=trials:{} target_clone_slots:{} min_iterations:{} max_iterations:{}",
        config.trials, config.target_clone_slots, config.min_iterations, config.max_iterations
    );
    println!(
        "segments\titerations\tclone_all_pub/s\tshared_prefix_pub/s\tclone_all_ns/pub\tshared_prefix_ns/pub\tspeedup\tsigned_delta"
    );

    for &segment_count in SEGMENT_COUNTS {
        let mappings = initial_mappings(segment_count);
        let clone_all = Arc::new(CloneAllView {
            mappings: mappings.clone().into_boxed_slice(),
        });
        let shared_prefix = Arc::new(SharedPrefixView {
            sealed: Arc::from(mappings[..segment_count - 1].to_vec()),
            tip: Arc::clone(&mappings[segment_count - 1]),
        });
        let tips = replacement_tips(segment_count);
        let iterations = config.iterations(segment_count);
        let mut clone_all_samples = Vec::with_capacity(config.trials);
        let mut shared_prefix_samples = Vec::with_capacity(config.trials);

        for trial in 0..config.trials {
            let (clone_all_elapsed, clone_all_checksum, shared_elapsed, shared_checksum) =
                if trial & 1 == 0 {
                    let (clone_elapsed, clone_checksum) =
                        measure_clone_all(Arc::clone(&clone_all), &tips, iterations);
                    let (shared_elapsed, shared_checksum) =
                        measure_shared_prefix(Arc::clone(&shared_prefix), &tips, iterations);
                    (
                        clone_elapsed,
                        clone_checksum,
                        shared_elapsed,
                        shared_checksum,
                    )
                } else {
                    let (shared_elapsed, shared_checksum) =
                        measure_shared_prefix(Arc::clone(&shared_prefix), &tips, iterations);
                    let (clone_elapsed, clone_checksum) =
                        measure_clone_all(Arc::clone(&clone_all), &tips, iterations);
                    (
                        clone_elapsed,
                        clone_checksum,
                        shared_elapsed,
                        shared_checksum,
                    )
                };
            assert_eq!(clone_all_checksum, shared_checksum);
            clone_all_samples.push(clone_all_elapsed);
            shared_prefix_samples.push(shared_elapsed);
        }

        let clone_all_ns = median_ns_per_publication(clone_all_samples, iterations);
        let shared_prefix_ns = median_ns_per_publication(shared_prefix_samples, iterations);
        let clone_all_per_second = 1_000_000_000.0 / clone_all_ns;
        let shared_prefix_per_second = 1_000_000_000.0 / shared_prefix_ns;
        let speedup = clone_all_ns / shared_prefix_ns;
        let signed_delta = (shared_prefix_per_second / clone_all_per_second - 1.0) * 100.0;
        println!(
            "{segment_count}\t{iterations}\t{clone_all_per_second:.0}\t{shared_prefix_per_second:.0}\t{clone_all_ns:.2}\t{shared_prefix_ns:.2}\t{speedup:.2}x\t{signed_delta:+.1}%"
        );
    }
}
