
use super::*;

#[test]
fn find_extracted_chain_acc_rejects_missing_or_ambiguous_files() {
    let temp = tempfile::tempdir().expect("temp");
    let missing = find_extracted_chain_acc(temp.path()).expect_err("missing chain.acc should fail");
    assert!(
        missing.to_string().contains("no chain*.acc"),
        "unexpected error: {missing}"
    );

    std::fs::write(temp.path().join("chain.0.acc"), b"").expect("first");
    std::fs::write(temp.path().join("chain.1.acc"), b"").expect("second");
    let ambiguous =
        find_extracted_chain_acc(temp.path()).expect_err("multiple chain.acc files should fail");
    assert!(
        ambiguous.to_string().contains("multiple chain*.acc"),
        "unexpected error: {ambiguous}"
    );
}

#[test]
fn command_preflight_reports_missing_binary_by_name() {
    let err = ensure_command_available("neo-rs-command-that-should-not-exist")
        .expect_err("missing extraction command must produce a deterministic error");

    assert!(
        err.to_string()
            .contains("required command `neo-rs-command-that-should-not-exist` is not available"),
        "unexpected error: {err}"
    );
}

#[test]
fn cached_extracted_chain_acc_requires_matching_package_md5_marker() {
    let temp = tempfile::tempdir().expect("temp");
    let extract_dir = temp.path().join("chain.0.acc");
    std::fs::create_dir_all(&extract_dir).expect("extract dir");
    std::fs::write(extract_dir.join("chain.0.acc"), b"old chain").expect("chain");
    std::fs::write(
        extract_dir.join(".neo-fast-sync-package-md5"),
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
    )
    .expect("old marker");

    let cached = cached_extracted_chain_acc(&extract_dir, "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB")
        .expect("check cached extract");

    assert!(
        cached.is_none(),
        "stale extracted chain.acc from a previous package MD5 must not be reused"
    );
}

#[test]
fn cached_extracted_chain_acc_rejects_empty_chain_file() {
    let temp = tempfile::tempdir().expect("temp");
    let extract_dir = temp.path().join("chain.0.acc");
    std::fs::create_dir_all(&extract_dir).expect("extract dir");
    std::fs::write(extract_dir.join("chain.0.acc"), b"").expect("empty chain");
    std::fs::write(
        extract_dir.join(".neo-fast-sync-package-md5"),
        "ABCDEF0123456789ABCDEF0123456789\n",
    )
    .expect("marker");

    let cached = cached_extracted_chain_acc(&extract_dir, "ABCDEF0123456789ABCDEF0123456789")
        .expect("check cached extract");

    assert!(
        cached.is_none(),
        "an empty extracted chain.acc should force re-extraction before import"
    );
}

#[test]
fn cached_extracted_chain_acc_requires_v2_marker_with_matching_size() {
    let temp = tempfile::tempdir().expect("temp");
    let extract_dir = temp.path().join("chain.0.acc");
    let chain_path = extract_dir.join("chain.0.acc");
    std::fs::create_dir_all(&extract_dir).expect("extract dir");
    std::fs::write(&chain_path, b"complete chain.acc").expect("chain");

    std::fs::write(
        extract_dir.join(".neo-fast-sync-package-md5"),
        "ABCDEF0123456789ABCDEF0123456789\n",
    )
    .expect("legacy marker");
    let cached = cached_extracted_chain_acc(&extract_dir, "ABCDEF0123456789ABCDEF0123456789")
        .expect("check legacy marker");
    assert!(
        cached.is_none(),
        "legacy MD5-only markers should force re-extraction before cache reuse"
    );

    write_extract_md5_marker(
        &extract_dir,
        "ABCDEF0123456789ABCDEF0123456789",
        &chain_path,
    )
    .expect("v2 marker");
    let cached = cached_extracted_chain_acc(&extract_dir, "ABCDEF0123456789ABCDEF0123456789")
        .expect("check v2 marker");
    assert_eq!(cached.as_deref(), Some(chain_path.as_path()));

    std::fs::write(&chain_path, b"truncated").expect("truncate chain");
    let cached = cached_extracted_chain_acc(&extract_dir, "ABCDEF0123456789ABCDEF0123456789")
        .expect("check size mismatch");
    assert!(
        cached.is_none(),
        "chain.acc size mismatch should force re-extraction before cache reuse"
    );
}

#[test]
fn failed_fast_sync_extract_removes_partial_extract_directory() {
    let temp = tempfile::tempdir().expect("temp");
    let zip_path = temp.path().join("chain.0.acc.zip");
    std::fs::write(&zip_path, b"not a zip archive").expect("bad zip");

    let err =
        ensure_chain_acc_extracted(&zip_path, temp.path(), "ABCDEF0123456789ABCDEF0123456789")
            .expect_err("invalid zip should fail extraction");

    assert!(
        err.to_string().contains("unzip failed"),
        "unexpected error: {err}"
    );
    assert!(
        !temp.path().join("chain.0.acc").exists(),
        "failed extraction must not leave a partial fast-sync extract directory"
    );
}
