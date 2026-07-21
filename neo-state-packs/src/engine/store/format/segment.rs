//! Single-segment format foundation and identity validation.

use super::*;

const SEGMENT_MAGIC: &[u8; 8] = b"N3PSEG01";
pub(super) const SEGMENT_HEADER_LEN: usize = PACK_SEGMENT_HEADER_LEN as usize;
const SEGMENT_HEADER_CHECKSUM_START: usize = 32;
const SEGMENT_PENDING_SUFFIX: &str = ".pending";

/// Returns the canonical path of an identified segment.
pub(super) fn segment_path(root: &Path, id: PackSegmentId) -> PathBuf {
    root.join(id.file_name())
}

/// Creates, authenticates, and durably publishes the initial segment.
pub(super) fn create_initial_segment(root: &Path) -> Result<(File, PathBuf)> {
    let id = PackSegmentId::INITIAL;
    let path = segment_path(root, id);
    let pending = pending_segment_path(&path);
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
    file.sync_all()
        .with_context(|| format!("sync pack segment header {}", pending.display()))?;
    drop(file);
    fs::rename(&pending, &path)
        .with_context(|| format!("publish initial pack segment {}", path.display()))?;
    sync_directory(root)?;
    let file = open_segment_for_append(&path)?;
    validate_segment_header(&file, &path, id)?;
    Ok((file, path))
}

/// Opens and authenticates the initial segment.
pub(super) fn open_initial_segment(root: &Path) -> Result<(File, PathBuf)> {
    let id = PackSegmentId::INITIAL;
    let path = segment_path(root, id);
    let file = open_segment_for_append(&path)?;
    validate_segment_header(&file, &path, id)?;
    Ok((file, path))
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

fn open_segment_for_append(path: &Path) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .append(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(path)
        .with_context(|| format!("open pack segment {}", path.display()))
}

fn pending_segment_path(path: &Path) -> PathBuf {
    let mut pending = path.as_os_str().to_os_string();
    pending.push(SEGMENT_PENDING_SUFFIX);
    PathBuf::from(pending)
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
        let reopened = open_segment_for_append(&path).expect("reopen segment");
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
}
