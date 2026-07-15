use super::*;

fn sample_manifest() -> SyncManifest {
    serde_json::from_str(
            r#"
{
  "n3mainnet": {
    "full": {
      "path": "https://packets.azureedge.net/neochain/n3mainnet/full/0-10/ABCDEF0123456789ABCDEF0123456789/chain.0.acc.zip",
      "md5": "abcdef0123456789abcdef0123456789",
      "start": 0,
      "end": 10
    }
  },
  "n3testnet": {
    "full": {
      "path": "https://packets.azureedge.net/neochain/n3testnet/full/0-20/11111111111111111111111111111111/chain.0.acc.zip",
      "md5": "11111111111111111111111111111111",
      "start": 0,
      "end": 20
    }
  }
}
"#,
        )
        .expect("sample manifest")
}

#[test]
fn selects_n3_mainnet_full_package_from_manifest() {
    let package = select_full_package(&sample_manifest(), MAINNET_MAGIC).expect("package");

    assert_eq!(package.network_key, "n3mainnet");
    assert_eq!(package.filename, "chain.0.acc.zip");
    assert_eq!(package.md5, "ABCDEF0123456789ABCDEF0123456789");
    assert_eq!(package.sha256, None);
    assert_eq!(package.start, 0);
    assert_eq!(package.end, 10);
}

#[test]
fn selects_package_with_optional_sha256_digest() {
    let manifest: SyncManifest = serde_json::from_str(
        r#"
{
  "n3mainnet": {
    "full": {
      "path": "https://packets.azureedge.net/neochain/n3mainnet/full/0-10/ABCDEF0123456789ABCDEF0123456789/chain.0.acc.zip",
      "md5": "abcdef0123456789abcdef0123456789",
      "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
      "start": 0,
      "end": 10
    }
  }
}
"#,
    )
    .expect("manifest");

    let package = select_full_package(&manifest, MAINNET_MAGIC).expect("package");
    assert_eq!(
        package.sha256.as_deref(),
        Some("0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF")
    );
}

#[test]
fn rejects_manifest_package_with_invalid_sha256_digest() {
    let manifest: SyncManifest = serde_json::from_str(
        r#"
{
  "n3mainnet": {
    "full": {
      "path": "https://packets.azureedge.net/neochain/n3mainnet/full/0-10/ABCDEF0123456789ABCDEF0123456789/chain.0.acc.zip",
      "md5": "abcdef0123456789abcdef0123456789",
      "sha256": "not-a-digest",
      "start": 0,
      "end": 10
    }
  }
}
"#,
    )
    .expect("manifest");

    let err = select_full_package(&manifest, MAINNET_MAGIC)
        .expect_err("invalid sha256 must fail closed");
    assert!(
        err.to_string().contains("sha256"),
        "unexpected error: {err}"
    );
}

#[test]
fn selects_n3_testnet_full_package_from_manifest() {
    let package = select_full_package(&sample_manifest(), TESTNET_MAGIC).expect("package");

    assert_eq!(package.network_key, "n3testnet");
    assert_eq!(package.end, 20);
}

#[test]
fn rejects_unsupported_network_magic() {
    let err = select_full_package(&sample_manifest(), 0x0102_0304)
        .expect_err("private network should not select official package");

    assert!(
        err.to_string().contains("MainNet/TestNet only"),
        "unexpected error: {err}"
    );
}

#[test]
fn rejects_manifest_package_with_reversed_height_range() {
    let manifest: SyncManifest = serde_json::from_str(
            r#"
{
  "n3mainnet": {
    "full": {
      "path": "https://packets.azureedge.net/neochain/n3mainnet/full/10-0/ABCDEF0123456789ABCDEF0123456789/chain.10.acc.zip",
      "md5": "abcdef0123456789abcdef0123456789",
      "start": 10,
      "end": 0
    }
  }
}
"#,
        )
        .expect("manifest");

    let err = select_full_package(&manifest, MAINNET_MAGIC)
        .expect_err("reversed fast-sync package range should fail");

    assert!(
        err.to_string().contains("start height"),
        "unexpected error: {err}"
    );
}

#[test]
fn rejects_manifest_package_with_non_https_url() {
    let manifest: SyncManifest = serde_json::from_str(
        r#"
{
  "n3mainnet": {
    "full": {
      "path": "file:///tmp/chain.0.acc.zip",
      "md5": "abcdef0123456789abcdef0123456789",
      "start": 0,
      "end": 10
    }
  }
}
"#,
    )
    .expect("manifest");

    let err = select_full_package(&manifest, MAINNET_MAGIC)
        .expect_err("non-HTTPS package URL should fail");

    assert!(
        err.to_string().contains("unsupported URL scheme"),
        "unexpected error: {err}"
    );
}

#[test]
fn rejects_manifest_package_with_cleartext_http_url() {
    let manifest: SyncManifest = serde_json::from_str(
        r#"
{
  "n3mainnet": {
    "full": {
      "path": "http://packets.example/chain.0.acc.zip",
      "md5": "abcdef0123456789abcdef0123456789",
      "start": 0,
      "end": 10
    }
  }
}
"#,
    )
    .expect("manifest");

    let err = select_full_package(&manifest, MAINNET_MAGIC)
        .expect_err("cleartext package URL should fail");

    assert!(
        err.to_string().contains("expected https"),
        "unexpected error: {err}"
    );
}

#[test]
fn rejects_non_https_final_response_url() {
    let url = url::Url::parse("http://packages.example/chain.0.acc.zip").unwrap();
    let err = ensure_https_url(&url, "fast-sync package")
        .expect_err("a redirect target must remain HTTPS");

    assert!(
        err.to_string().contains("resolved to non-HTTPS URL"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn secure_client_rejects_cleartext_requests_before_connecting() {
    let err = secure_http_client()
        .expect("secure client")
        .get("http://127.0.0.1:1/chain.0.acc.zip")
        .send()
        .await
        .expect_err("HTTPS-only client must reject cleartext URLs");

    assert!(err.is_builder(), "unexpected error: {err}");
}
