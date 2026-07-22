use super::*;

/// One-copy builder for an append frame backed by borrowed operation values.
///
/// Values are copied directly into their final value section; callers do not
/// allocate an intermediate [`PackOperation`]. Builders are created only by
/// [`PackStore::frame_builder`] or [`PackStore::frame_builder_with_value_bytes`],
/// binding every allocation to that store's identity and configured limits.
/// The exact layout is revalidated before any durable bytes are written by
/// [`PackStore::prepare_built_append`].
#[derive(Debug)]
pub struct PackFrameBuilder {
    store_instance_id: u64,
    context: PackFrameContext,
    expected_rows: usize,
    expected_value_bytes: Option<usize>,
    metadata_bytes: u64,
    max_frame_payload_bytes: u64,
    max_pending_bytes: u64,
    values: Vec<u8>,
    rows: Vec<PendingFrameRow>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PackFrameBuilderLayout {
    pub(super) rows: usize,
    pub(super) metadata_bytes: u64,
    pub(super) value_bytes: u64,
    pub(super) distinct_keys: usize,
}

impl PackFrameBuilder {
    pub(super) fn for_store(
        store_instance_id: u64,
        context: PackFrameContext,
        expected_rows: usize,
        value_bytes: Option<u64>,
        max_frame_payload_bytes: u64,
        max_pending_bytes: u64,
    ) -> Result<Self> {
        validate_frame_context(context)?;
        ensure!(expected_rows > 0, "frame must contain at least one row");
        let operation_count =
            u64::try_from(expected_rows).context("frame row count overflows u64")?;
        ensure!(
            operation_count <= MAX_FRAME_ROWS,
            "frame row count exceeds the hard limit of {MAX_FRAME_ROWS}"
        );
        let metadata_bytes = expected_rows
            .checked_mul(FRAME_ROW_METADATA_LEN)
            .context("frame metadata size overflows usize")?;
        let metadata_bytes =
            u64::try_from(metadata_bytes).context("frame metadata size overflows u64")?;
        let expected_value_bytes = value_bytes
            .map(|bytes| {
                usize::try_from(bytes).context("aggregate frame value bytes overflow usize")
            })
            .transpose()?;
        let initial_capacity = expected_value_bytes.unwrap_or(0);
        ensure_builder_layout_limits(
            metadata_bytes,
            u64::try_from(initial_capacity).context("frame value size overflows u64")?,
            max_frame_payload_bytes,
            max_pending_bytes,
        )?;

        let mut values = Vec::new();
        values
            .try_reserve_exact(initial_capacity)
            .context("reserve frame values")?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(expected_rows)
            .context("reserve frame rows")?;
        Ok(Self {
            store_instance_id,
            context,
            expected_rows,
            expected_value_bytes,
            metadata_bytes,
            max_frame_payload_bytes,
            max_pending_bytes,
            values,
            rows,
        })
    }

    /// Encodes one borrowed operation directly into the final frame sections.
    ///
    /// `Some(value)` is a put, including an empty put; `None` is a tombstone.
    /// Keys must have the exact fixed width and be supplied in non-decreasing
    /// order. Duplicate keys retain insertion order through their sequence.
    #[inline]
    pub fn push(&mut self, key: &[u8], value: Option<&[u8]>) -> Result<()> {
        let key: [u8; PACK_KEY_BYTES] = key.try_into().with_context(|| {
            format!(
                "pack key has {} bytes, expected {PACK_KEY_BYTES}",
                key.len()
            )
        })?;
        self.push_key(key, value)
    }

    /// Encodes one operation whose fixed-width key was already validated.
    #[inline]
    pub fn push_key(&mut self, key: [u8; PACK_KEY_BYTES], value: Option<&[u8]>) -> Result<()> {
        ensure!(
            key[0] == FRAME_NODE_KEY_PREFIX,
            "frame key is outside the MPT node namespace"
        );
        ensure!(
            self.rows.len() < self.expected_rows,
            "frame builder received more than its declared row count"
        );
        if let Some(previous) = self.rows.last() {
            ensure!(
                previous.key <= key,
                "borrowed frame keys must be supplied in non-decreasing order"
            );
        }

        let tombstone = value.is_none();
        let value = value.unwrap_or_default();
        let value_len = u32::try_from(value.len()).context("frame value exceeds u32")?;
        let next_value_len = self
            .values
            .len()
            .checked_add(value.len())
            .context("frame value size overflows usize")?;
        if let Some(expected_value_bytes) = self.expected_value_bytes {
            ensure!(
                next_value_len <= expected_value_bytes,
                "frame values exceed the declared aggregate byte count"
            );
        }
        ensure_builder_layout_limits(
            self.metadata_bytes,
            u64::try_from(next_value_len).context("frame value size overflows u64")?,
            self.max_frame_payload_bytes,
            self.max_pending_bytes,
        )?;
        if self.expected_value_bytes.is_none()
            && self.values.capacity() - self.values.len() < value.len()
        {
            self.values
                .try_reserve(value.len())
                .context("grow frame values")?;
        }

        let sequence = u32::try_from(self.rows.len()).context("frame sequence exceeds u32")?;
        let relative_value_offset = if tombstone {
            0
        } else {
            u64::try_from(self.values.len()).context("frame value offset overflows u64")?
        };
        self.values.extend_from_slice(value);
        self.rows.push(PendingFrameRow {
            key,
            sequence,
            value_offset: relative_value_offset,
            value_len,
            tombstone,
        });
        Ok(())
    }

    /// Number of operations encoded so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no operations have been encoded yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub(super) const fn store_instance_id(&self) -> u64 {
        self.store_instance_id
    }

    /// Validates the final layout without allocating metadata or index rows.
    pub(super) fn preflight(&self) -> Result<PackFrameBuilderLayout> {
        ensure!(
            self.rows.len() == self.expected_rows,
            "frame builder encoded {} rows, expected {}",
            self.rows.len(),
            self.expected_rows
        );
        if let Some(expected_value_bytes) = self.expected_value_bytes {
            ensure!(
                self.values.len() == expected_value_bytes,
                "frame builder encoded {} value bytes, expected {}",
                self.values.len(),
                expected_value_bytes
            );
        }
        let distinct_keys = 1 + self
            .rows
            .windows(2)
            .filter(|rows| rows[0].key != rows[1].key)
            .count();
        Ok(PackFrameBuilderLayout {
            rows: self.expected_rows,
            metadata_bytes: self.metadata_bytes,
            value_bytes: u64::try_from(self.values.len())
                .context("frame value size overflows u64")?,
            distinct_keys,
        })
    }

    pub(super) fn finish(
        self,
    ) -> Result<(PackFrameContext, usize, Vec<u8>, Vec<u8>, Vec<IndexEntry>)> {
        let _ = self.preflight()?;
        let (metadata, values, entries) = encode_pending_rows(self.rows, self.values)?;
        Ok((self.context, self.expected_rows, metadata, values, entries))
    }
}

fn ensure_builder_layout_limits(
    metadata_bytes: u64,
    value_bytes: u64,
    max_frame_payload_bytes: u64,
    max_pending_bytes: u64,
) -> Result<()> {
    let payload_bytes = metadata_bytes
        .checked_add(value_bytes)
        .context("frame payload size overflows u64")?;
    if payload_bytes > max_frame_payload_bytes {
        return Err(PackStoreError::LimitExceeded {
            limit: PackStoreLimit::FramePayloadBytes,
            actual: payload_bytes,
            maximum: max_frame_payload_bytes,
        }
        .into());
    }
    let frame_bytes = payload_bytes
        .checked_add(FRAME_HEADER_LEN as u64)
        .and_then(|bytes| bytes.checked_add(FRAME_FOOTER_LEN as u64))
        .context("encoded frame length overflows")?;
    if frame_bytes > max_pending_bytes {
        return Err(PackStoreError::LimitExceeded {
            limit: PackStoreLimit::PendingBytes,
            actual: frame_bytes,
            maximum: max_pending_bytes,
        }
        .into());
    }
    Ok(())
}
