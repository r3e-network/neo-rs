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
async fn fast_sync_download_retries_transient_failure_with_clean_partial_file() {
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
                        assert!(
                            !destination.exists(),
                            "partial file from the failed attempt must be removed before retry"
                        );
                        std::fs::write(&destination, b"complete package").expect("complete write");
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
        b"complete package"
    );
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
