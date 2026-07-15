use super::*;

#[test]
fn failed_fast_sync_download_cleanup_removes_partial_without_touching_cached_zip() {
    let temp = tempfile::tempdir().expect("temp");
    let partial = temp.path().join("chain.0.acc.zip.part");
    let cached = temp.path().join("chain.0.acc.zip");
    std::fs::write(&partial, b"incomplete package").expect("partial");
    std::fs::write(&cached, b"previous cached package").expect("cached");

    remove_partial_download(&partial).expect("cleanup partial");

    assert!(!partial.exists(), "partial download must be removed");
    assert_eq!(
        std::fs::read(&cached).expect("cached package remains"),
        b"previous cached package"
    );
}

#[tokio::test]
async fn fast_sync_download_retries_transient_failure_without_losing_partial_file() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let temp = tempfile::tempdir().expect("temp");
    let partial = temp.path().join("chain.0.acc.zip.part");
    let attempts = Arc::new(AtomicUsize::new(0));

    download_package_with_retries("https://example.invalid/chain.0.acc.zip", &partial, 2, {
        let attempts = Arc::clone(&attempts);
        move |_url, destination| {
            let attempts = Arc::clone(&attempts);
            async move {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                match attempt {
                    1 => {
                        std::fs::write(&destination, b"incomplete package").expect("partial write");
                        anyhow::bail!("simulated response EOF")
                    }
                    2 => {
                        assert_eq!(
                            std::fs::read(&destination).expect("resumable partial"),
                            b"incomplete package",
                            "retry must retain bytes from the failed attempt"
                        );
                        let mut file = std::fs::OpenOptions::new()
                            .append(true)
                            .open(&destination)
                            .expect("open partial for resume");
                        file.write_all(b" remainder").expect("append remainder");
                        Ok(())
                    }
                    other => panic!("unexpected download attempt {other}"),
                }
            }
        }
    })
    .await
    .expect("second download attempt should succeed");

    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(
        std::fs::read(&partial).expect("downloaded package"),
        b"incomplete package remainder"
    );
}

#[tokio::test]
async fn exhausted_download_attempts_preserve_partial_for_process_restart() {
    let temp = tempfile::tempdir().expect("temp");
    let partial = temp.path().join("chain.0.acc.zip.part");

    download_package_with_retries(
        "https://example.invalid/chain.0.acc.zip",
        &partial,
        1,
        |_url, destination| async move {
            std::fs::write(destination, b"resumable prefix").expect("partial write");
            anyhow::bail!("simulated connection loss")
        },
    )
    .await
    .expect_err("exhausted download must report its connection failure");

    assert_eq!(
        std::fs::read(partial).expect("partial survives process-level failure"),
        b"resumable prefix"
    );
}

#[test]
fn partial_content_plan_appends_only_at_the_requested_offset() {
    let plan = download_plan(
        reqwest::StatusCode::PARTIAL_CONTENT,
        10,
        Some(5),
        Some("bytes 10-14/15"),
    )
    .expect("valid resume response");

    assert_eq!(
        plan,
        DownloadPlan {
            append: true,
            starting_bytes: 10,
            expected_total_bytes: Some(15),
        }
    );
    assert!(
        download_plan(
            reqwest::StatusCode::PARTIAL_CONTENT,
            10,
            Some(5),
            Some("bytes 9-13/15"),
        )
        .is_err()
    );
}

#[test]
fn full_response_plan_safely_restarts_when_server_ignores_range() {
    let plan = download_plan(reqwest::StatusCode::OK, 10, Some(15), None)
        .expect("full response is a safe restart");

    assert_eq!(
        plan,
        DownloadPlan {
            append: false,
            starting_bytes: 0,
            expected_total_bytes: Some(15),
        }
    );
}

#[test]
fn parses_satisfied_and_unsatisfied_content_ranges() {
    assert_eq!(
        parse_content_range("bytes 10-14/15").expect("content range"),
        ContentRange {
            start: 10,
            end: 14,
            total: 15,
        }
    );
    assert_eq!(parse_unsatisfied_content_range("bytes */15"), Some(15));
    assert!(parse_content_range("items 10-14/15").is_err());
    assert!(parse_content_range("bytes 10-15/15").is_err());
}

#[test]
fn downloaded_content_length_mismatch_is_an_error() {
    let err = validate_downloaded_content_length("https://example.invalid/chain.zip", Some(12), 8)
        .expect_err("short fast-sync package download must fail");

    assert!(
        err.to_string().contains("content length mismatch"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string()
            .contains("expected 12 bytes, downloaded 8 bytes"),
        "unexpected error: {err}"
    );
}

#[test]
fn package_content_length_must_fit_the_configured_and_disk_limit() {
    let err = validate_download_limits("https://example.invalid/chain.zip", Some(13), 12)
        .expect_err("oversized package must fail before download");
    assert!(err.to_string().contains("12-byte download limit"));

    let err = validate_download_limits("https://example.invalid/chain.zip", None, 0)
        .expect_err("zero available bytes must fail before download");
    assert!(err.to_string().contains("no disk space"));
}

#[test]
fn package_byte_limit_is_finite_and_rejects_invalid_configuration() {
    assert_eq!(
        parse_package_byte_limit(None).expect("default limit"),
        DEFAULT_MAX_FAST_SYNC_PACKAGE_BYTES
    );
    assert_eq!(
        parse_package_byte_limit(Some(std::ffi::OsStr::new("4096"))).expect("configured limit"),
        4096
    );
    assert!(parse_package_byte_limit(Some(std::ffi::OsStr::new("0"))).is_err());
    assert!(parse_package_byte_limit(Some(std::ffi::OsStr::new("many"))).is_err());
}

#[test]
fn cache_volume_space_is_detected_before_download() {
    let temp = tempfile::tempdir().expect("temp");
    let destination = temp.path().join("chain.0.acc.zip.part");
    assert!(
        available_space_for(&destination).expect("available space") > 0,
        "test volume should report usable space"
    );
}

#[test]
fn parses_md5sum_digest_output() {
    let digest = parse_md5_digest_output(
        "md5sum",
        Path::new("chain.0.acc.zip"),
        b"abcdef0123456789abcdef0123456789  chain.0.acc.zip\n",
    )
    .expect("parse md5sum output");

    assert_eq!(digest, "ABCDEF0123456789ABCDEF0123456789");
}

#[test]
fn parses_bsd_md5_digest_output() {
    let digest = parse_md5_digest_output(
        "md5",
        Path::new("chain.0.acc.zip"),
        b"abcdef0123456789abcdef0123456789\n",
    )
    .expect("parse md5 output");

    assert_eq!(digest, "ABCDEF0123456789ABCDEF0123456789");
}

#[test]
fn parses_sha256sum_and_openssl_digest_output() {
    let digest = parse_digest_output(
        "sha256sum",
        Path::new("chain.0.acc.zip"),
        b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  chain.0.acc.zip\n",
        DigestKind::Sha256,
    )
    .expect("parse sha256sum");
    assert_eq!(
        digest,
        "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF"
    );

    let openssl = parse_digest_output(
        "openssl",
        Path::new("chain.0.acc.zip"),
        b"SHA256(chain.0.acc.zip)= 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n",
        DigestKind::Sha256,
    )
    .expect("parse openssl dgst");
    assert_eq!(
        openssl,
        "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF"
    );
}

#[test]
fn package_digest_validation_requires_sha256_when_present_and_fails_closed() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("chain.0.acc.zip");
    std::fs::write(&path, b"trusted-package-bytes").expect("write package");

    let md5 = read_digest(&path, DigestKind::Md5).expect("md5");
    let sha256 = read_digest(&path, DigestKind::Sha256).expect("sha256");

    let mut package = FastSyncPackage {
        network_key: "n3mainnet",
        url: "https://example.invalid/chain.0.acc.zip".to_string(),
        md5: md5.clone(),
        sha256: Some(sha256.clone()),
        start: 0,
        end: 10,
        filename: "chain.0.acc.zip".to_string(),
    };
    validate_package_digests(&path, &package).expect("matching digests must accept");

    package.sha256 = Some("0".repeat(64));
    let err = validate_package_digests(&path, &package)
        .expect_err("wrong SHA-256 must fail closed before promotion");
    assert!(
        err.to_string().contains("SHA-256 mismatch"),
        "unexpected error: {err}"
    );

    // MD5-only packages must not promote: SHA-256 is mandatory authenticity.
    package.sha256 = None;
    package.md5 = md5;
    let err = validate_package_digests(&path, &package)
        .expect_err("MD5-only packages must fail closed before promotion");
    assert!(
        err.to_string().contains("missing a SHA-256"),
        "unexpected error: {err}"
    );
}
