use super::*;

#[test]
fn log_format_accepts_supported_values() {
    assert!(matches!(log_format(None).unwrap(), LogFormat::Pretty));
    assert!(matches!(
        log_format(Some("compact")).unwrap(),
        LogFormat::Compact
    ));
    assert!(matches!(log_format(Some("json")).unwrap(), LogFormat::Json));
    assert!(log_format(Some("yaml")).is_err());
}

#[test]
fn logging_filter_uses_toml_level_when_rust_log_is_unset() {
    let config = LoggingSection {
        level: Some("warn,neo=debug".to_string()),
        ..Default::default()
    };
    assert_eq!(logging_filter_directive(&config, None), "warn,neo=debug");
    assert_eq!(
        logging_filter_directive(&config, Some("error,neo_rpc=trace")),
        "error,neo_rpc=trace"
    );
}

#[test]
fn max_file_size_parser_accepts_common_units() {
    let config = LoggingSection {
        max_file_size: Some("100MB".to_string()),
        ..Default::default()
    };
    assert_eq!(
        config.max_file_size_bytes().unwrap(),
        Some(100 * 1024 * 1024)
    );

    let config = LoggingSection {
        max_file_size: Some("1_024 bytes".to_string()),
        ..Default::default()
    };
    assert_eq!(config.max_file_size_bytes().unwrap(), Some(1024));

    let config = LoggingSection {
        max_file_size: Some("2 GiB".to_string()),
        ..Default::default()
    };
    assert_eq!(
        config.max_file_size_bytes().unwrap(),
        Some(2 * 1024 * 1024 * 1024)
    );
}

#[test]
fn size_rotating_writer_rolls_active_file_and_retains_archives() {
    let temp = tempfile::tempdir().expect("temp log dir");
    let path = temp.path().join("neo-node.log");
    let mut writer =
        SizeRotatingFileWriter::open(&path, 8, 2).expect("open rotating log writer");

    writer.write_all(b"12345678").expect("write first line");
    writer
        .write_all(b"abc")
        .expect("rotate and write second line");
    writer
        .write_all(b"defghijk")
        .expect("rotate and write third line");
    writer
        .write_all(b"z")
        .expect("rotate and write fourth line");
    writer.flush().expect("flush writer");

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "z");
    assert_eq!(
        std::fs::read_to_string(archive_path(&path, 1)).unwrap(),
        "defghijk"
    );
    assert_eq!(
        std::fs::read_to_string(archive_path(&path, 2)).unwrap(),
        "abc"
    );
    assert!(!archive_path(&path, 3).exists());
}
