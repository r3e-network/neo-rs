use super::*;

/// One-copy builder for an append frame backed by borrowed operation values.
///
/// Values are copied directly into their final frame-payload representation;
/// callers do not need to allocate an intermediate [`PackOperation`]. The
/// expected row count is exact and is revalidated before any durable bytes are
/// written by [`PackStore::prepare_built_append`].
#[derive(Debug)]
pub struct PackFrameBuilder {
    expected_rows: usize,
    expected_payload_bytes: Option<usize>,
    payload: Vec<u8>,
    entries: Vec<IndexEntry>,
    keys_are_sorted: bool,
}

impl PackFrameBuilder {
    /// Creates a frame builder for exactly `expected_rows` operations.
    pub fn new(expected_rows: usize) -> Result<Self> {
        Self::with_optional_value_bytes(expected_rows, None)
    }

    /// Creates a builder with an exact aggregate borrowed-value byte count.
    ///
    /// Supplying the count performs one payload allocation and adds a final
    /// fail-closed check that the visited values match the caller's materialized
    /// overlay accounting.
    pub fn with_value_bytes(expected_rows: usize, value_bytes: u64) -> Result<Self> {
        Self::with_optional_value_bytes(expected_rows, Some(value_bytes))
    }

    fn with_optional_value_bytes(expected_rows: usize, value_bytes: Option<u64>) -> Result<Self> {
        ensure!(expected_rows > 0, "frame must contain at least one row");
        let operation_count =
            u64::try_from(expected_rows).context("frame row count overflows u64")?;
        ensure!(
            operation_count <= MAX_FRAME_ROWS,
            "frame row count exceeds the hard limit of {MAX_FRAME_ROWS}"
        );
        let minimum_payload = expected_rows
            .checked_mul(PACK_KEY_BYTES + 1 + 4)
            .context("minimum frame payload size overflows usize")?;
        let expected_payload_bytes = value_bytes
            .map(|value_bytes| {
                usize::try_from(value_bytes)
                    .context("aggregate frame value bytes overflow usize")
                    .and_then(|value_bytes| {
                        minimum_payload
                            .checked_add(value_bytes)
                            .context("frame payload size overflows usize")
                    })
            })
            .transpose()?;
        let initial_capacity = expected_payload_bytes.unwrap_or(minimum_payload);
        ensure!(
            u64::try_from(initial_capacity).context("frame payload size overflows u64")?
                <= MAX_FRAME_PAYLOAD_BYTES,
            "frame payload exceeds the hard limit of {MAX_FRAME_PAYLOAD_BYTES} bytes"
        );

        let mut payload = Vec::new();
        payload
            .try_reserve_exact(initial_capacity)
            .context("reserve frame payload")?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(expected_rows)
            .context("reserve frame index entries")?;
        Ok(Self {
            expected_rows,
            expected_payload_bytes,
            payload,
            entries,
            keys_are_sorted: true,
        })
    }

    /// Encodes one borrowed operation directly into the final frame payload.
    ///
    /// `Some(value)` is a put (including an empty value); `None` is a
    /// tombstone. Keys must have the pack format's exact fixed width.
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
            self.entries.len() < self.expected_rows,
            "frame builder received more than its declared row count"
        );
        let tombstone = value.is_none();
        let value = value.unwrap_or_default();
        let value_len = u32::try_from(value.len()).context("frame value exceeds u32")?;
        let row_bytes = PACK_KEY_BYTES
            .checked_add(1 + 4)
            .and_then(|bytes| bytes.checked_add(value.len()))
            .context("frame row size overflows usize")?;
        let next_payload_len = self
            .payload
            .len()
            .checked_add(row_bytes)
            .context("frame payload size overflows usize")?;
        if let Some(expected_payload_bytes) = self.expected_payload_bytes {
            ensure!(
                next_payload_len <= expected_payload_bytes,
                "frame values exceed the declared aggregate byte count"
            );
        } else {
            ensure!(
                u64::try_from(next_payload_len).context("frame payload size overflows u64")?
                    <= MAX_FRAME_PAYLOAD_BYTES,
                "frame payload exceeds the hard limit of {MAX_FRAME_PAYLOAD_BYTES} bytes"
            );
        }
        if self.expected_payload_bytes.is_none()
            && self.payload.capacity() - self.payload.len() < row_bytes
        {
            self.payload
                .try_reserve(row_bytes)
                .context("grow frame payload")?;
        }

        if let Some(previous) = self.entries.last() {
            self.keys_are_sorted &= previous.key <= key;
        }
        let sequence = u32::try_from(self.entries.len()).context("frame sequence exceeds u32")?;
        self.payload.extend_from_slice(&key);
        self.payload.push(u8::from(!tombstone));
        self.payload.extend_from_slice(&value_len.to_le_bytes());
        let relative_value_offset =
            u64::try_from(self.payload.len()).context("frame value offset overflows u64")?;
        self.payload.extend_from_slice(value);
        self.entries.push(IndexEntry {
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
        self.entries.len()
    }

    /// Whether no operations have been encoded yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn finish(
        mut self,
        frame_start: u64,
    ) -> Result<(usize, Vec<u8>, Vec<IndexEntry>, bool)> {
        ensure!(
            self.entries.len() == self.expected_rows,
            "frame builder encoded {} rows, expected {}",
            self.entries.len(),
            self.expected_rows
        );
        if let Some(expected_payload_bytes) = self.expected_payload_bytes {
            ensure!(
                self.payload.len() == expected_payload_bytes,
                "frame builder encoded {} payload bytes, expected {}",
                self.payload.len(),
                expected_payload_bytes
            );
        }
        let payload_start = frame_start
            .checked_add(FRAME_HEADER_LEN as u64)
            .context("frame payload offset overflows u64")?;
        for entry in &mut self.entries {
            entry.value_offset = payload_start
                .checked_add(entry.value_offset)
                .context("absolute frame value offset overflows u64")?;
        }
        Ok((
            self.expected_rows,
            self.payload,
            self.entries,
            self.keys_are_sorted,
        ))
    }
}
