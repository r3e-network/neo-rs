//! Immutable positioned-read view over authenticated pack segment prefixes.

use super::segment::{segment_path, validate_segment_header};
use super::*;

/// One exact committed prefix of an authenticated pack segment.
#[derive(Debug)]
pub(super) struct SegmentMapping {
    id: PackSegmentId,
    path: PathBuf,
    map: Mmap,
    lookup_map: Option<Mmap>,
}

impl SegmentMapping {
    /// Adopts mappings that were built while authenticating or publishing a
    /// segment. Both mappings must cover exactly the same committed prefix.
    pub(super) fn from_maps(
        id: PackSegmentId,
        path: PathBuf,
        map: Mmap,
        lookup_map: Option<Mmap>,
    ) -> Result<Self> {
        if let Some(lookup_map) = lookup_map.as_ref() {
            ensure!(
                lookup_map.as_slice().len() == map.as_slice().len(),
                "segment {id} lookup mapping length differs from its committed mapping"
            );
        }
        Ok(Self {
            id,
            path,
            map,
            lookup_map,
        })
    }

    pub(super) fn map_file(
        id: PackSegmentId,
        file: &File,
        path: &Path,
        committed_bytes: u64,
        random_lookup: bool,
    ) -> Result<Self> {
        let map = Mmap::map(file, committed_bytes, path)?;
        let lookup_map = random_lookup
            .then(|| Mmap::map_random(file, committed_bytes, path))
            .transpose()?;
        Self::from_maps(id, path.to_path_buf(), map, lookup_map)
    }

    const fn id(&self) -> PackSegmentId {
        self.id
    }

    fn committed_bytes(&self) -> usize {
        self.map.as_slice().len()
    }

    fn lookup_map(&self) -> &Mmap {
        self.lookup_map.as_ref().unwrap_or(&self.map)
    }
}

/// One manifest generation's complete immutable segment view.
///
/// Sealed mappings are ordered by durable segment identity and shared across
/// successor generations. The writable tip is separate so ordinary frame
/// publication replaces one `Arc` without copying the complete segment list.
/// A snapshot owns one `Arc<SegmentSet>`, so positioned reads cannot drift to
/// a later generation while compaction or publication replaces the live view.
#[derive(Debug)]
pub(super) struct SegmentSet {
    sealed: Arc<[Arc<SegmentMapping>]>,
    tip: Arc<SegmentMapping>,
}

impl SegmentSet {
    /// Opens the exact committed prefixes selected by authenticated manifest
    /// extents. An empty store still pins the initial segment header.
    pub(super) fn open(
        root: &Path,
        extents: &[ManifestExtent],
        options: PackStoreOptions,
        max_segment_bytes: u64,
    ) -> Result<Self> {
        let selected: Vec<_> = if extents.is_empty() {
            vec![(PackSegmentId::INITIAL, PACK_SEGMENT_HEADER_LEN)]
        } else {
            extents
                .iter()
                .map(|extent| (extent.segment_id, extent.frame_end))
                .collect()
        };
        let mut mappings = Vec::with_capacity(selected.len());
        for (id, committed_bytes) in selected {
            if committed_bytes > max_segment_bytes {
                return Err(PackStoreError::LimitExceeded {
                    limit: PackStoreLimit::SegmentBytes,
                    actual: committed_bytes,
                    maximum: max_segment_bytes,
                }
                .into());
            }
            let path = segment_path(root, id);
            let file = File::open(&path)
                .with_context(|| format!("open committed segment {}", path.display()))?;
            validate_segment_header(&file, &path, id)?;
            ensure!(
                file.metadata()
                    .with_context(|| format!("stat committed segment {}", path.display()))?
                    .len()
                    >= committed_bytes,
                "segment {id} is shorter than its selected committed prefix"
            );
            mappings.push(Arc::new(SegmentMapping::map_file(
                id,
                &file,
                &path,
                committed_bytes,
                options.random_point_mmap,
            )?));
        }
        Self::new(mappings)
    }

    pub(super) fn new(mut mappings: Vec<Arc<SegmentMapping>>) -> Result<Self> {
        ensure!(!mappings.is_empty(), "segment set is empty");
        ensure!(
            mappings[0].id() == PackSegmentId::INITIAL,
            "segment set does not start at segment zero"
        );
        let mut previous: Option<PackSegmentId> = None;
        for mapping in &mappings {
            if let Some(previous) = previous {
                ensure!(
                    previous.checked_next() == Some(mapping.id()),
                    "segment set identities are not consecutive"
                );
            }
            let committed_bytes = u64::try_from(mapping.committed_bytes())
                .context("segment committed byte count does not fit u64")?;
            ensure!(
                committed_bytes >= PACK_SEGMENT_HEADER_LEN,
                "segment {} committed prefix is shorter than its header",
                mapping.id()
            );
            ensure!(
                mapping.id() == PackSegmentId::INITIAL || committed_bytes > PACK_SEGMENT_HEADER_LEN,
                "non-initial segment {} has no committed frame",
                mapping.id()
            );
            previous = Some(mapping.id());
        }
        let tip = mappings.pop().expect("validated segment set is non-empty");
        Ok(Self {
            sealed: Arc::from(mappings),
            tip,
        })
    }

    /// Builds a successor view by replacing the current tip prefix or adding
    /// exactly its next segment. Historical mappings stay shared.
    pub(super) fn with_replaced_or_appended(&self, mapping: Arc<SegmentMapping>) -> Result<Self> {
        if mapping.id() == self.tip.id() {
            ensure!(
                mapping.committed_bytes() > self.tip.committed_bytes(),
                "successor mapping for segment {} does not extend its committed prefix",
                mapping.id()
            );
            return Ok(Self {
                sealed: Arc::clone(&self.sealed),
                tip: mapping,
            });
        }

        ensure!(
            self.tip.id().checked_next() == Some(mapping.id()),
            "successor segment {} does not replace or follow current tip {}",
            mapping.id(),
            self.tip.id()
        );
        ensure!(
            mapping.committed_bytes() > PACK_SEGMENT_HEADER_LEN as usize,
            "successor segment {} has no committed frame",
            mapping.id()
        );
        let mut sealed = Vec::with_capacity(self.sealed.len().saturating_add(1));
        sealed.extend(self.sealed.iter().cloned());
        sealed.push(Arc::clone(&self.tip));
        Ok(Self {
            sealed: Arc::from(sealed),
            tip: mapping,
        })
    }

    pub(super) fn tip_id(&self) -> PackSegmentId {
        self.tip.id()
    }

    pub(super) fn tip_committed_bytes(&self) -> u64 {
        self.committed_bytes(self.tip_id())
            .expect("validated segment set contains its tip")
    }

    #[cfg(test)]
    fn contains(&self, id: PackSegmentId) -> bool {
        self.mapping(id).is_ok()
    }

    pub(super) fn committed_bytes(&self, id: PackSegmentId) -> Result<u64> {
        u64::try_from(self.mapping(id)?.committed_bytes())
            .context("segment committed byte count does not fit u64")
    }

    pub(super) fn validate_range(&self, position: PackPosition, length: u32) -> Result<()> {
        let mapping = self.mapping(position.segment())?;
        ensure!(
            position.offset() >= PACK_SEGMENT_HEADER_LEN,
            "positioned range in segment {} starts inside its header",
            position.segment()
        );
        let end = position
            .offset()
            .checked_add(u64::from(length))
            .context("positioned segment range overflows")?;
        let committed_bytes = u64::try_from(mapping.committed_bytes())
            .context("segment committed byte count does not fit u64")?;
        ensure!(
            position.offset() < committed_bytes && end <= committed_bytes,
            "positioned range in segment {} at offset {} ends at {end}, beyond its committed length of {committed_bytes} bytes",
            position.segment(),
            position.offset()
        );
        Ok(())
    }

    /// Creates an independent sparse-read view of the same immutable prefixes.
    /// Evidence callers may release these pages without perturbing mappings
    /// pinned by live readers and snapshots.
    pub(super) fn dedicated_random_view(&self) -> Result<Self> {
        let mut mappings = Vec::with_capacity(self.len());
        for mapping in self.iter() {
            let file = File::open(&mapping.path).with_context(|| {
                format!(
                    "open segment {} for dedicated random reads",
                    mapping.path.display()
                )
            })?;
            validate_segment_header(&file, &mapping.path, mapping.id())?;
            let committed_bytes = u64::try_from(mapping.committed_bytes())
                .context("segment committed byte count does not fit u64")?;
            ensure!(
                file.metadata()
                    .with_context(|| format!("stat segment {}", mapping.path.display()))?
                    .len()
                    >= committed_bytes,
                "segment {} became shorter than its pinned committed prefix",
                mapping.id()
            );
            mappings.push(Arc::new(SegmentMapping::from_maps(
                mapping.id(),
                mapping.path.clone(),
                Mmap::map_random(&file, committed_bytes, &mapping.path)?,
                None,
            )?));
        }
        Self::new(mappings)
    }

    pub(super) fn lookup_slice(&self, position: PackPosition, length: u32) -> Result<&[u8]> {
        self.validate_range(position, length)?;
        let mapping = self.mapping(position.segment())?;
        positioned_slice(mapping.lookup_map(), position, length)
    }

    pub(super) fn committed_slice(&self, position: PackPosition, length: u32) -> Result<&[u8]> {
        self.validate_range(position, length)?;
        let mapping = self.mapping(position.segment())?;
        positioned_slice(&mapping.map, position, length)
    }

    pub(super) fn sequential_mappings(&self) -> SequentialSegmentMappings<'_> {
        SequentialSegmentMappings {
            segments: self,
            next_index: 0,
        }
    }

    pub(super) fn reclaim_random_lookup_pages(&self) -> Result<()> {
        for mapping in self.iter() {
            if let Some(map) = mapping.lookup_map.as_ref() {
                let _ = map.advise_dontneed(0, map.as_slice().len())?;
            }
        }
        Ok(())
    }

    /// Releases every page in an independently mapped validation view.
    pub(super) fn reclaim_all_pages(&self) -> Result<()> {
        for mapping in self.iter() {
            let _ = mapping
                .map
                .advise_dontneed(0, mapping.map.as_slice().len())?;
            if let Some(map) = mapping.lookup_map.as_ref() {
                let _ = map.advise_dontneed(0, map.as_slice().len())?;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn has_random_lookup(&self, id: PackSegmentId) -> bool {
        self.mapping(id)
            .is_ok_and(|mapping| mapping.lookup_map.is_some())
    }

    fn mapping(&self, id: PackSegmentId) -> Result<&SegmentMapping> {
        if id == self.tip.id() {
            return Ok(self.tip.as_ref());
        }
        let index = usize::try_from(id.get()).context("segment identity does not fit usize")?;
        let mapping = self
            .sealed
            .get(index)
            .with_context(|| format!("position names unavailable segment {id}"))?;
        ensure!(
            mapping.id() == id,
            "position names unavailable segment {id}"
        );
        Ok(mapping.as_ref())
    }

    fn len(&self) -> usize {
        self.sealed.len().saturating_add(1)
    }

    fn iter(&self) -> impl Iterator<Item = &Arc<SegmentMapping>> {
        self.sealed.iter().chain(std::iter::once(&self.tip))
    }

    fn mapping_at(&self, index: usize) -> Option<&SegmentMapping> {
        if index == self.sealed.len() {
            Some(self.tip.as_ref())
        } else {
            self.sealed.get(index).map(AsRef::as_ref)
        }
    }
}

pub(super) struct SequentialSegmentMappings<'a> {
    segments: &'a SegmentSet,
    next_index: usize,
}

impl Iterator for SequentialSegmentMappings<'_> {
    type Item = Result<SequentialSegmentMapping>;

    fn next(&mut self) -> Option<Self::Item> {
        let mapping = self.segments.mapping_at(self.next_index)?;
        self.next_index = self.next_index.saturating_add(1);
        Some(open_sequential_mapping(mapping))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.segments.len().saturating_sub(self.next_index);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for SequentialSegmentMappings<'_> {}

pub(super) struct SequentialSegmentMapping {
    pub(super) id: PackSegmentId,
    pub(super) map: Mmap,
}

fn open_sequential_mapping(mapping: &SegmentMapping) -> Result<SequentialSegmentMapping> {
    let file = File::open(&mapping.path).with_context(|| {
        format!(
            "open segment {} for sequential validation",
            mapping.path.display()
        )
    })?;
    validate_segment_header(&file, &mapping.path, mapping.id())?;
    let committed_bytes = u64::try_from(mapping.committed_bytes())
        .context("segment committed byte count does not fit u64")?;
    ensure!(
        file.metadata()
            .with_context(|| format!("stat segment {}", mapping.path.display()))?
            .len()
            >= committed_bytes,
        "segment {} became shorter than its pinned committed prefix",
        mapping.id()
    );
    Ok(SequentialSegmentMapping {
        id: mapping.id(),
        map: Mmap::map_sequential(&file, committed_bytes, &mapping.path)?,
    })
}

fn positioned_slice(map: &Mmap, position: PackPosition, length: u32) -> Result<&[u8]> {
    let start =
        usize::try_from(position.offset()).context("positioned offset does not fit usize")?;
    let length = usize::try_from(length).context("positioned length does not fit usize")?;
    let end = start
        .checked_add(length)
        .context("positioned range does not fit usize")?;
    map.as_slice()
        .get(start..end)
        .context("validated positioned range is absent from its segment mapping")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapped_fixture(root: &Path, id: PackSegmentId, committed_bytes: u64) -> Arc<SegmentMapping> {
        let path = root.join(id.file_name());
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .expect("create segment fixture");
        file.set_len(committed_bytes).expect("size segment fixture");
        Arc::new(
            SegmentMapping::map_file(id, &file, &path, committed_bytes, true).expect("map fixture"),
        )
    }

    #[test]
    fn equal_offsets_resolve_inside_the_named_segment() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut mappings = Vec::new();
        for (id, value) in [
            (PackSegmentId::INITIAL, b"zero".as_slice()),
            (PackSegmentId::new(1), b"one!".as_slice()),
        ] {
            let path = root.path().join(id.file_name());
            let file = OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .open(&path)
                .expect("create segment fixture");
            file.set_len(PACK_SEGMENT_HEADER_LEN + 4)
                .expect("size segment fixture");
            file.write_all_at(value, PACK_SEGMENT_HEADER_LEN)
                .expect("write segment fixture");
            mappings.push(Arc::new(
                SegmentMapping::map_file(id, &file, &path, PACK_SEGMENT_HEADER_LEN + 4, true)
                    .expect("map fixture"),
            ));
        }
        let segments = SegmentSet::new(mappings).expect("segment set");
        let offset = PACK_SEGMENT_HEADER_LEN;
        assert_eq!(
            segments
                .lookup_slice(PackPosition::new(PackSegmentId::INITIAL, offset), 4)
                .expect("first positioned read"),
            b"zero"
        );
        assert_eq!(
            segments
                .lookup_slice(PackPosition::new(PackSegmentId::new(1), offset), 4)
                .expect("second positioned read"),
            b"one!"
        );
        assert_eq!(
            segments
                .lookup_slice(PackPosition::new(PackSegmentId::INITIAL, offset), 0)
                .expect("zero-length value before EOF"),
            b""
        );
        assert!(
            segments
                .lookup_slice(PackPosition::new(PackSegmentId::INITIAL, offset + 4), 0)
                .expect_err("zero-length value at EOF must fail")
                .to_string()
                .contains("beyond its committed length")
        );
        assert!(segments.contains(PackSegmentId::new(1)));
        assert!(
            segments
                .lookup_slice(PackPosition::new(PackSegmentId::new(2), offset), 4)
                .expect_err("unknown segment must fail")
                .to_string()
                .contains("unavailable segment")
        );
        assert!(
            segments
                .lookup_slice(PackPosition::new(PackSegmentId::INITIAL, offset - 1), 0)
                .expect_err("segment-header position must fail")
                .to_string()
                .contains("inside its header")
        );
    }

    #[test]
    fn same_tip_successor_reuses_the_sealed_prefix() {
        let root = tempfile::tempdir().expect("tempdir");
        let first = mapped_fixture(
            root.path(),
            PackSegmentId::INITIAL,
            PACK_SEGMENT_HEADER_LEN + 4,
        );
        let second = mapped_fixture(
            root.path(),
            PackSegmentId::new(1),
            PACK_SEGMENT_HEADER_LEN + 4,
        );
        let segments =
            SegmentSet::new(vec![Arc::clone(&first), Arc::clone(&second)]).expect("segment set");
        let path = root.path().join(PackSegmentId::new(1).file_name());
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .expect("open tip fixture");
        file.set_len(PACK_SEGMENT_HEADER_LEN + 8)
            .expect("extend tip fixture");
        let extended = Arc::new(
            SegmentMapping::map_file(
                PackSegmentId::new(1),
                &file,
                &path,
                PACK_SEGMENT_HEADER_LEN + 8,
                true,
            )
            .expect("map extended tip"),
        );

        let successor = segments
            .with_replaced_or_appended(Arc::clone(&extended))
            .expect("replace tip");

        assert!(Arc::ptr_eq(&segments.sealed, &successor.sealed));
        assert!(Arc::ptr_eq(&successor.sealed[0], &first));
        assert!(Arc::ptr_eq(&successor.tip, &extended));
        assert_eq!(successor.tip_committed_bytes(), PACK_SEGMENT_HEADER_LEN + 8);
    }

    #[test]
    fn rotation_moves_the_old_tip_into_a_new_sealed_prefix() {
        let root = tempfile::tempdir().expect("tempdir");
        let first = mapped_fixture(
            root.path(),
            PackSegmentId::INITIAL,
            PACK_SEGMENT_HEADER_LEN + 4,
        );
        let segments = SegmentSet::new(vec![Arc::clone(&first)]).expect("segment set");
        let second = mapped_fixture(
            root.path(),
            PackSegmentId::new(1),
            PACK_SEGMENT_HEADER_LEN + 4,
        );

        let successor = segments
            .with_replaced_or_appended(Arc::clone(&second))
            .expect("append next segment");

        assert_eq!(successor.sealed.len(), 1);
        assert!(Arc::ptr_eq(&successor.sealed[0], &first));
        assert!(Arc::ptr_eq(&successor.tip, &second));
    }

    #[test]
    fn successor_rejects_non_growing_or_empty_segment_prefixes() {
        let root = tempfile::tempdir().expect("tempdir");
        let first = mapped_fixture(
            root.path(),
            PackSegmentId::INITIAL,
            PACK_SEGMENT_HEADER_LEN + 4,
        );
        let segments = SegmentSet::new(vec![Arc::clone(&first)]).expect("segment set");
        assert!(
            segments
                .with_replaced_or_appended(first)
                .expect_err("equal tip prefix must fail")
                .to_string()
                .contains("does not extend")
        );
        let empty_next =
            mapped_fixture(root.path(), PackSegmentId::new(1), PACK_SEGMENT_HEADER_LEN);
        assert!(
            segments
                .with_replaced_or_appended(empty_next)
                .expect_err("header-only successor must fail")
                .to_string()
                .contains("no committed frame")
        );
    }
}
