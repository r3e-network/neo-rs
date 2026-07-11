use tempfile::tempdir;

use crate::{StaticFileArchiveFactory, StaticFileError, StaticFileProviderFactory};

#[test]
fn writer_lease_is_held_until_the_last_archive_clone_drops() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let factory = StaticFileArchiveFactory::default();
    let archive = factory.open(&path).expect("first writer");
    let clone = archive.clone();

    assert!(
        factory.open(&path).is_err(),
        "a second writer must not open the same archive"
    );
    drop(archive);
    assert!(
        factory.open(&path).is_err(),
        "a provider clone must keep the writer lease alive"
    );
    drop(clone);

    factory
        .open(&path)
        .expect("kernel lease should release with the final clone");
}

#[cfg(unix)]
#[test]
fn writer_lease_canonicalizes_symlinked_archive_paths() {
    let temp = tempdir().expect("tempdir");
    let real = temp.path().join("real");
    std::fs::create_dir(&real).expect("real archive directory");
    let alias = temp.path().join("alias");
    std::os::unix::fs::symlink(&real, &alias).expect("archive directory symlink");
    let factory = StaticFileArchiveFactory::default();
    let archive = factory
        .open(&real.join("ledger.static"))
        .expect("open canonical path");

    assert!(matches!(
        factory.open(&alias.join("ledger.static")),
        Err(StaticFileError::WriterOwned { .. })
    ));
    drop(archive);
    factory
        .open(&alias.join("ledger.static"))
        .expect("alias should open after lease release");
}

#[test]
fn writer_lease_follows_hard_link_file_identity() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("ledger.static");
    let alias = temp.path().join("ledger-alias.static");
    let factory = StaticFileArchiveFactory::default();
    let archive = factory.open(&path).expect("open archive");
    std::fs::hard_link(&path, &alias).expect("hard-link archive alias");

    assert!(matches!(
        factory.open(&alias),
        Err(StaticFileError::WriterOwned { .. })
    ));
    drop(archive);
    factory
        .open(&alias)
        .expect("hard-link alias should open after lease release");
}
