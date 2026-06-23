const RUNTIME_SOURCES: &[(&str, &str)] = &[
    ("indexer.rs", include_str!("../indexer.rs")),
    ("service.rs", include_str!("../service.rs")),
];

#[test]
fn indexer_runtime_sources_do_not_panic_on_recoverable_state() {
    for (name, source) in RUNTIME_SOURCES {
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        for forbidden in [".expect(", ".unwrap(", "panic!", "todo!", "unimplemented!"] {
            assert!(
                !production.contains(forbidden),
                "{name} production path should return IndexerError instead of using {forbidden}"
            );
        }
    }
}
