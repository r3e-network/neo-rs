//! Segment publication, canonical discovery, and identity validation.

use super::*;
use crate::engine::failpoint;
use std::ffi::OsStr;

const SEGMENT_MAGIC: &[u8; 8] = b"N3PSEG01";
pub(super) const SEGMENT_HEADER_LEN: usize = PACK_SEGMENT_HEADER_LEN as usize;
const SEGMENT_HEADER_CHECKSUM_START: usize = 32;
const SEGMENT_PENDING_SUFFIX: &str = ".pending";

/// One canonical segment identity discovered without opening or mutating it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SegmentCatalogEntry {
    pub(super) id: PackSegmentId,
    pub(super) path: PathBuf,
}

/// Returns the canonical path of an identified segment.
pub(super) fn segment_path(root: &Path, id: PackSegmentId) -> PathBuf {
    root.join(id.file_name())
}

/// Returns the unpublished path used while constructing one segment header.
pub(super) fn pending_segment_path(root: &Path, id: PackSegmentId) -> PathBuf {
    let path = segment_path(root, id);
    let mut pending = path.as_os_str().to_os_string();
    pending.push(SEGMENT_PENDING_SUFFIX);
    PathBuf::from(pending)
}

/// Parses an exact pending segment name emitted by [`create_segment`].
pub(super) fn parse_pending_segment_file_name(name: &OsStr) -> Option<PackSegmentId> {
    let bytes = name.as_encoded_bytes();
    let segment_name = bytes.strip_suffix(SEGMENT_PENDING_SUFFIX.as_bytes())?;
    let segment_name = std::str::from_utf8(segment_name).ok()?;
    PackSegmentId::from_file_name(OsStr::new(segment_name))
}

/// Creates, authenticates, and durably publishes one identified segment.
///
/// The caller holds the store writer lease. A header is first written to an
/// identity-specific pending file, synced completely, renamed to its final
/// canonical name, and fenced by syncing the store directory.
pub(super) fn create_segment(root: &Path, id: PackSegmentId) -> Result<(File, PathBuf)> {
    let path = segment_path(root, id);
    ensure!(
        !path
            .try_exists()
            .with_context(|| format!("inspect pack segment {}", path.display()))?,
        "pack segment {} already exists",
        path.display()
    );
    let pending = pending_segment_path(root, id);
    let file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(&pending)
        .with_context(|| format!("create pending pack segment {}", pending.display()))?;
    let header = encode_segment_header(id);
    file.write_all_at(&header, 0)
        .with_context(|| format!("write pack segment header {}", pending.display()))?;
    failpoint::crash("segment.header.before-sync");
    file.sync_all()
        .with_context(|| format!("sync pack segment header {}", pending.display()))?;
    failpoint::crash("segment.header.after-sync");
    drop(file);
    ensure!(
        !path
            .try_exists()
            .with_context(|| format!("inspect pack segment {}", path.display()))?,
        "pack segment {} appeared while its header was pending",
        path.display()
    );
    fs::rename(&pending, &path)
        .with_context(|| format!("publish pack segment {}", path.display()))?;
    failpoint::crash("segment.header.after-rename");
    sync_directory(root)?;
    failpoint::crash("segment.header.after-directory-sync");
    open_segment_for_append(root, id)
}

/// Opens and authenticates one identified segment for appends.
pub(super) fn open_segment_for_append(root: &Path, id: PackSegmentId) -> Result<(File, PathBuf)> {
    let path = segment_path(root, id);
    let file = OpenOptions::new()
        .read(true)
        .append(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(&path)
        .with_context(|| format!("open pack segment {} for append", path.display()))?;
    validate_segment_header(&file, &path, id)?;
    Ok((file, path))
}

/// Opens and authenticates one identified segment without write access.
pub(super) fn open_segment_read_only(root: &Path, id: PackSegmentId) -> Result<(File, PathBuf)> {
    let path = segment_path(root, id);
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(&path)
        .with_context(|| format!("open pack segment {} read-only", path.display()))?;
    validate_segment_header(&file, &path, id)?;
    Ok((file, path))
}

/// Discovers exact canonical segment names in ascending identity order.
///
/// Discovery is intentionally read-only and does not authenticate bytes. The
/// authoritative recovery horizon chooses the required prefix, whose files
/// are then opened and authenticated independently. Canonical names bound to
/// non-regular filesystem entries fail closed instead of being ignored.
pub(super) fn discover_segment_catalog(root: &Path) -> Result<Vec<SegmentCatalogEntry>> {
    let mut segments = Vec::new();
    let entries = fs::read_dir(root)
        .with_context(|| format!("read pack segment directory {}", root.display()))?;
    for entry in entries {
        let entry = entry.context("read pack segment directory entry")?;
        let Some(id) = PackSegmentId::from_file_name(&entry.file_name()) else {
            continue;
        };
        ensure!(
            entry
                .file_type()
                .with_context(|| format!("classify pack segment {}", entry.path().display()))?
                .is_file(),
            "canonical pack segment {} is not a regular file",
            entry.path().display()
        );
        ensure_segment_catalog_slot(segments.len())?;
        segments.push(SegmentCatalogEntry {
            id,
            path: entry.path(),
        });
    }
    segments.sort_unstable_by_key(|segment| segment.id);
    for pair in segments.windows(2) {
        ensure!(
            pair[0].id != pair[1].id,
            "duplicate canonical pack segment identity {}",
            pair[0].id
        );
    }
    Ok(segments)
}

/// Bounds directory discovery before retaining another path or opening any
/// segment bytes. The writer applies the same manifest-extent cap before it
/// creates a new canonical segment.
fn ensure_segment_catalog_slot(current: usize) -> Result<()> {
    let next = current
        .checked_add(1)
        .context("pack segment count overflows")?;
    if next > manifest::HARD_MAX_MANIFEST_EXTENTS {
        return Err(PackStoreError::LimitExceeded {
            limit: PackStoreLimit::Segments,
            actual: u64::try_from(next).context("pack segment count does not fit u64")?,
            maximum: manifest::HARD_MAX_MANIFEST_EXTENTS as u64,
        }
        .into());
    }
    Ok(())
}

/// Selects the exact consecutive segment-zero-through-tip catalog prefix.
///
/// Segments after `required_tip` are intentionally not constrained here: they
/// may be an orphan suffix that recovery can classify after authenticating the
/// committed prefix.
pub(super) fn required_segment_prefix(
    segments: &[SegmentCatalogEntry],
    required_tip: PackSegmentId,
) -> Result<&[SegmentCatalogEntry]> {
    let mut expected = PackSegmentId::INITIAL;
    for (index, segment) in segments.iter().enumerate() {
        if segment.id > required_tip {
            break;
        }
        ensure!(
            segment.id == expected,
            "required pack segment {expected} is missing before discovered segment {}",
            segment.id
        );
        if segment.id == required_tip {
            return Ok(&segments[..=index]);
        }
        expected = expected
            .checked_next()
            .context("required pack segment identity overflows")?;
    }
    Err(anyhow::anyhow!(
        "required pack segment {required_tip} is missing"
    ))
}

/// Creates, authenticates, and durably publishes the initial segment.
pub(super) fn create_initial_segment(root: &Path) -> Result<(File, PathBuf)> {
    create_segment(root, PackSegmentId::INITIAL)
}

#[cfg(test)]
fn open_initial_segment(root: &Path) -> Result<(File, PathBuf)> {
    open_segment_for_append(root, PackSegmentId::INITIAL)
}

/// Returns whether the canonical initial segment is present.
pub(crate) fn initial_segment_exists(root: &Path) -> bool {
    segment_path(root, PackSegmentId::INITIAL).exists()
}

/// Validates one segment header against its expected durable identity.
pub(super) fn validate_segment_header(
    file: &File,
    path: &Path,
    expected_id: PackSegmentId,
) -> Result<()> {
    let len = file
        .metadata()
        .with_context(|| format!("stat pack segment {}", path.display()))?
        .len();
    ensure!(
        len >= SEGMENT_HEADER_LEN as u64,
        "pack segment {} has a truncated header",
        path.display()
    );
    let mut header = [0u8; SEGMENT_HEADER_LEN];
    file.read_exact_at(&mut header, 0)
        .with_context(|| format!("read pack segment header {}", path.display()))?;
    ensure!(
        &header[..8] == SEGMENT_MAGIC,
        "pack segment {} has invalid magic",
        path.display()
    );
    let version = u32::from_le_bytes(header[8..12].try_into().expect("segment version range"));
    if version != PACK_SEGMENT_FORMAT_VERSION {
        return Err(PackStoreError::unsupported_version(
            PackStoreArtifact::Segment,
            version,
            &[PACK_SEGMENT_FORMAT_VERSION],
        )
        .into());
    }
    let header_len = u32::from_le_bytes(
        header[12..16]
            .try_into()
            .expect("segment header length range"),
    );
    ensure!(
        header_len == SEGMENT_HEADER_LEN as u32,
        "pack segment {} declares header length {header_len}; expected {SEGMENT_HEADER_LEN}",
        path.display()
    );
    let actual_id = PackSegmentId::new(u64::from_le_bytes(
        header[16..24].try_into().expect("segment identity range"),
    ));
    ensure!(
        actual_id == expected_id,
        "pack segment {} identity {actual_id} differs from expected {expected_id}",
        path.display()
    );
    ensure!(
        header[24..32] == [0; 8],
        "pack segment {} reserved header bytes are non-zero",
        path.display()
    );
    let expected_checksum = digest(&header[..SEGMENT_HEADER_CHECKSUM_START]);
    ensure!(
        header[SEGMENT_HEADER_CHECKSUM_START..] == expected_checksum,
        "pack segment {} header checksum mismatch",
        path.display()
    );
    Ok(())
}

fn encode_segment_header(id: PackSegmentId) -> [u8; SEGMENT_HEADER_LEN] {
    let mut header = [0u8; SEGMENT_HEADER_LEN];
    header[..8].copy_from_slice(SEGMENT_MAGIC);
    header[8..12].copy_from_slice(&PACK_SEGMENT_FORMAT_VERSION.to_le_bytes());
    header[12..16].copy_from_slice(&(SEGMENT_HEADER_LEN as u32).to_le_bytes());
    header[16..24].copy_from_slice(&id.get().to_le_bytes());
    let checksum = digest(&header[..SEGMENT_HEADER_CHECKSUM_START]);
    header[SEGMENT_HEADER_CHECKSUM_START..].copy_from_slice(&checksum);
    header
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_header_binds_version_identity_and_checksum() {
        let root = tempfile::tempdir().expect("tempdir");
        let (file, path) = create_initial_segment(root.path()).expect("create initial segment");
        validate_segment_header(&file, &path, PackSegmentId::INITIAL).expect("validate header");
        assert!(validate_segment_header(&file, &path, PackSegmentId::new(1)).is_err());

        let mut header = [0u8; SEGMENT_HEADER_LEN];
        file.read_exact_at(&mut header, 0).expect("read header");
        header[24] = 1;
        drop(file);
        let writable = OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open writable");
        writable.write_all_at(&header, 0).expect("corrupt header");
        writable.sync_all().expect("sync corruption");
        drop(writable);
        let reopened = OpenOptions::new()
            .read(true)
            .open(&path)
            .expect("reopen segment");
        assert!(
            validate_segment_header(&reopened, &path, PackSegmentId::INITIAL)
                .expect_err("reserved bytes must fail")
                .to_string()
                .contains("reserved")
        );
    }

    #[test]
    fn unknown_segment_version_fails_closed() {
        let root = tempfile::tempdir().expect("tempdir");
        let (file, path) = create_initial_segment(root.path()).expect("create initial segment");
        let mut header = [0u8; SEGMENT_HEADER_LEN];
        file.read_exact_at(&mut header, 0).expect("read header");
        header[8..12].copy_from_slice(&(PACK_SEGMENT_FORMAT_VERSION + 1).to_le_bytes());
        let checksum = digest(&header[..SEGMENT_HEADER_CHECKSUM_START]);
        header[SEGMENT_HEADER_CHECKSUM_START..].copy_from_slice(&checksum);
        drop(file);
        let writable = OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open writable");
        writable.write_all_at(&header, 0).expect("write version");
        writable.sync_all().expect("sync version");
        drop(writable);
        let error = open_initial_segment(root.path()).expect_err("unknown version must fail");
        assert!(matches!(
            error.downcast_ref::<PackStoreError>(),
            Some(PackStoreError::UnsupportedVersion {
                artifact: PackStoreArtifact::Segment,
                found,
                ..
            }) if *found == PACK_SEGMENT_FORMAT_VERSION + 1
        ));
    }

    #[test]
    fn arbitrary_segment_is_published_from_one_canonical_pending_name() {
        let root = tempfile::tempdir().expect("tempdir");
        let id = PackSegmentId::new(7);
        let (append, path) = create_segment(root.path(), id).expect("create segment");

        assert_eq!(path, segment_path(root.path(), id));
        assert!(!pending_segment_path(root.path(), id).exists());
        validate_segment_header(&append, &path, id).expect("validate append handle");
        let (read_only, read_path) =
            open_segment_read_only(root.path(), id).expect("open read-only");
        assert_eq!(read_path, path);
        validate_segment_header(&read_only, &read_path, id).expect("validate read-only handle");
    }

    #[test]
    fn duplicate_segment_create_does_not_replace_published_bytes() {
        let root = tempfile::tempdir().expect("tempdir");
        let id = PackSegmentId::new(3);
        let (file, path) = create_segment(root.path(), id).expect("create segment");
        file.write_all_at(b"sentinel", SEGMENT_HEADER_LEN as u64)
            .expect("append sentinel");
        file.sync_all().expect("sync sentinel");
        drop(file);
        let before = fs::read(&path).expect("read segment before duplicate create");

        let error = create_segment(root.path(), id).expect_err("duplicate create must fail");

        assert!(error.to_string().contains("already exists"));
        assert_eq!(
            fs::read(&path).expect("read segment after duplicate create"),
            before
        );
        assert!(!pending_segment_path(root.path(), id).exists());
    }

    #[test]
    fn canonical_discovery_is_sorted_read_only_and_ignores_pending_aliases() {
        let root = tempfile::tempdir().expect("tempdir");
        for id in [PackSegmentId::new(2), PackSegmentId::INITIAL] {
            drop(create_segment(root.path(), id).expect("create segment"));
        }
        fs::write(
            pending_segment_path(root.path(), PackSegmentId::new(3)),
            b"pending",
        )
        .expect("write pending segment");
        fs::write(root.path().join("frames-2.pack"), b"alias").expect("write alias");
        fs::write(root.path().join("unrelated.pack"), b"unrelated").expect("write unrelated");

        let discovered = discover_segment_catalog(root.path()).expect("discover segments");

        assert_eq!(
            discovered
                .iter()
                .map(|segment| segment.id)
                .collect::<Vec<_>>(),
            vec![PackSegmentId::INITIAL, PackSegmentId::new(2)]
        );
        assert!(pending_segment_path(root.path(), PackSegmentId::new(3)).exists());
    }

    #[test]
    fn canonical_discovery_rejects_non_regular_segment_entries() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = segment_path(root.path(), PackSegmentId::INITIAL);
        fs::create_dir(&path).expect("create canonical segment directory");

        let error = discover_segment_catalog(root.path())
            .expect_err("canonical non-file must fail discovery");
        assert!(error.to_string().contains("not a regular file"));
    }

    #[test]
    fn canonical_discovery_capacity_fails_before_retaining_an_extra_segment() {
        let error = ensure_segment_catalog_slot(manifest::HARD_MAX_MANIFEST_EXTENTS)
            .expect_err("one segment above the hard manifest capacity must fail");
        assert!(matches!(
            error.downcast_ref::<PackStoreError>(),
            Some(PackStoreError::LimitExceeded {
                limit: PackStoreLimit::Segments,
                actual,
                maximum,
            }) if *actual == *maximum + 1
                && *maximum == manifest::HARD_MAX_MANIFEST_EXTENTS as u64
        ));
    }

    #[test]
    fn required_prefix_rejects_missing_and_duplicate_identities() {
        let entry = |id| SegmentCatalogEntry {
            id,
            path: PathBuf::from(id.file_name()),
        };
        let complete = vec![
            entry(PackSegmentId::INITIAL),
            entry(PackSegmentId::new(1)),
            entry(PackSegmentId::new(2)),
            entry(PackSegmentId::new(4)),
        ];
        assert_eq!(
            required_segment_prefix(&complete, PackSegmentId::new(2))
                .expect("complete required prefix"),
            &complete[..3]
        );

        for invalid in [
            vec![entry(PackSegmentId::new(1))],
            vec![entry(PackSegmentId::INITIAL), entry(PackSegmentId::new(2))],
            vec![
                entry(PackSegmentId::INITIAL),
                entry(PackSegmentId::INITIAL),
                entry(PackSegmentId::new(1)),
            ],
        ] {
            assert!(
                required_segment_prefix(&invalid, PackSegmentId::new(1)).is_err(),
                "invalid required prefix was accepted: {invalid:?}"
            );
        }
    }

    #[test]
    fn pending_segment_parser_accepts_only_exact_canonical_names() {
        let id = PackSegmentId::new(42);
        let canonical = format!("{}.pending", id.file_name());
        assert_eq!(
            parse_pending_segment_file_name(OsStr::new(&canonical)),
            Some(id)
        );
        for name in [
            "frames-42.pack.pending",
            "frames-00000000000000000042.pack.tmp",
            "frames-00000000000000000042.pack.pending.extra",
            "Frames-00000000000000000042.pack.pending",
        ] {
            assert_eq!(parse_pending_segment_file_name(OsStr::new(name)), None);
        }
    }
}
