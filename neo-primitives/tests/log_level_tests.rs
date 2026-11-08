use neo_primitives::LogLevel;

#[test]
fn log_level_values_match_c_sharp() {
    assert_eq!(LogLevel::Debug as u8, 0);
    assert_eq!(LogLevel::Info as u8, 1);
    assert_eq!(LogLevel::Warning as u8, 2);
    assert_eq!(LogLevel::Error as u8, 3);
    assert_eq!(LogLevel::Fatal as u8, 4);
}

#[test]
fn log_level_constants_match_enum_values() {
    assert_eq!(LogLevel::DEBUG_LEVEL, 0);
    assert_eq!(LogLevel::INFO_LEVEL, 1);
    assert_eq!(LogLevel::WARNING_LEVEL, 2);
    assert_eq!(LogLevel::ERROR_LEVEL, 3);
    assert_eq!(LogLevel::FATAL_LEVEL, 4);
}

#[test]
fn log_level_ordering() {
    assert!(LogLevel::Debug < LogLevel::Info);
    assert!(LogLevel::Info < LogLevel::Warning);
    assert!(LogLevel::Warning < LogLevel::Error);
    assert!(LogLevel::Error < LogLevel::Fatal);
}

#[test]
fn log_level_equality() {
    assert_eq!(LogLevel::Debug, LogLevel::Debug);
    assert_ne!(LogLevel::Debug, LogLevel::Info);
}

#[test]
fn log_level_clone() {
    let level = LogLevel::Warning;
    let cloned = level.clone();
    assert_eq!(level, cloned);
}

#[test]
fn log_level_copy() {
    let level = LogLevel::Error;
    let copied = level;
    assert_eq!(level, copied);
}

#[test]
fn log_level_debug_format() {
    assert_eq!(format!("{:?}", LogLevel::Debug), "Debug");
    assert_eq!(format!("{:?}", LogLevel::Info), "Info");
    assert_eq!(format!("{:?}", LogLevel::Warning), "Warning");
    assert_eq!(format!("{:?}", LogLevel::Error), "Error");
    assert_eq!(format!("{:?}", LogLevel::Fatal), "Fatal");
}

#[test]
fn log_level_comparison_with_same_level() {
    assert!(LogLevel::Info >= LogLevel::Info);
    assert!(LogLevel::Info <= LogLevel::Info);
}

#[test]
fn log_level_all_variants_are_ordered() {
    let mut levels = vec![
        LogLevel::Fatal,
        LogLevel::Debug,
        LogLevel::Warning,
        LogLevel::Info,
        LogLevel::Error,
    ];
    levels.sort();

    assert_eq!(
        levels,
        vec![
            LogLevel::Debug,
            LogLevel::Info,
            LogLevel::Warning,
            LogLevel::Error,
            LogLevel::Fatal
        ]
    );
}

#[test]
fn log_level_max_and_min() {
    use std::cmp::{max, min};

    assert_eq!(max(LogLevel::Debug, LogLevel::Fatal), LogLevel::Fatal);
    assert_eq!(min(LogLevel::Debug, LogLevel::Fatal), LogLevel::Debug);
    assert_eq!(max(LogLevel::Warning, LogLevel::Info), LogLevel::Warning);
}

#[test]
fn log_level_enum_discriminants_are_sequential() {
    assert_eq!(LogLevel::Info as u8, (LogLevel::Debug as u8) + 1);
    assert_eq!(LogLevel::Warning as u8, (LogLevel::Info as u8) + 1);
    assert_eq!(LogLevel::Error as u8, (LogLevel::Warning as u8) + 1);
    assert_eq!(LogLevel::Fatal as u8, (LogLevel::Error as u8) + 1);
}
