use super::*;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeBindings {
    pub transaction: String,
    pub context: String,
    pub build: String,
    pub budget: String,
}

impl ProbeBindings {
    pub fn calculate(job: &ProbeJob) -> Result<Self, WorkerError> {
        Ok(Self {
            transaction: job.transaction.content_hash()?,
            context: job.context.content_hash()?,
            build: job.build.content_hash()?,
            budget: job.budget.content_hash()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotEntry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub constant: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotInput {
    pub root_hash: String,
    pub height: u32,
    pub entries: Vec<SnapshotEntry>,
}

impl SnapshotInput {
    #[must_use]
    pub fn from_job(job: &ProbeJob, entries: Vec<SnapshotEntry>) -> Self {
        Self {
            root_hash: job.context.tip_hash.clone(),
            height: job.context.observed_height,
            entries,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeRequestFrame {
    pub job: ProbeJob,
    pub bindings: ProbeBindings,
    pub snapshot: SnapshotInput,
}

impl ProbeRequestFrame {
    pub fn new(job: ProbeJob, snapshot: SnapshotInput) -> Result<Self, WorkerError> {
        let bindings = ProbeBindings::calculate(&job)?;
        Ok(Self {
            job,
            bindings,
            snapshot,
        })
    }
}

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("probe frame length must be non-zero")]
    ZeroLengthFrame,
    #[error("probe frame exceeds 512 KiB")]
    FrameTooLarge,
    #[error("probe frame is truncated or has trailing bytes")]
    FrameBoundary,
    #[error("probe JSON contains trailing bytes")]
    TrailingBytes,
    #[error("probe JSON serialization failed")]
    Serialization(#[from] serde_json::Error),
    #[error("probe I/O failed")]
    Io(#[from] std::io::Error),
    #[error("probe job is invalid")]
    Contract(#[from] ProbeContractError),
    #[error("probe {0} binding is zero or mismatched")]
    BindingMismatch(&'static str),
    #[error("only transaction preflight jobs are accepted")]
    UnsupportedAction,
    #[error("detached snapshot root is invalid or mismatched")]
    SnapshotRootMismatch,
    #[error("detached snapshot height is mismatched")]
    SnapshotHeightMismatch,
    #[error("detached snapshot is invalid")]
    Snapshot(#[from] DetachedSnapshotError),
    #[error("disposable overlay rejected the deterministic write")]
    Overlay,
    #[error("disposable overlay changed detached backing state")]
    IsolationViolation,
    #[error("probe outcome exceeds its admitted budget")]
    BudgetViolation,
}
pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, WorkerError> {
    let payload = serde_json::to_vec(value)?;
    if payload.is_empty() {
        return Err(WorkerError::ZeroLengthFrame);
    }
    if payload.len() > MAX_FRAME_BYTES {
        return Err(WorkerError::FrameTooLarge);
    }
    let length = u32::try_from(payload.len()).map_err(|_| WorkerError::FrameTooLarge)?;
    let mut framed = Vec::with_capacity(4 + payload.len());
    framed.extend_from_slice(&length.to_be_bytes());
    framed.extend_from_slice(&payload);
    Ok(framed)
}
pub fn decode_frame<T: DeserializeOwned>(framed: &[u8]) -> Result<T, WorkerError> {
    let header: [u8; 4] = framed
        .get(..4)
        .ok_or(WorkerError::FrameBoundary)?
        .try_into()
        .map_err(|_| WorkerError::FrameBoundary)?;
    let length =
        usize::try_from(u32::from_be_bytes(header)).map_err(|_| WorkerError::FrameTooLarge)?;
    validate_frame_length(length)?;
    if framed.len() != 4 + length {
        return Err(WorkerError::FrameBoundary);
    }
    decode_payload(&framed[4..])
}
pub(super) fn read_frame(reader: &mut impl Read) -> Result<Vec<u8>, WorkerError> {
    let mut header = [0u8; 4];
    reader.read_exact(&mut header)?;
    let length =
        usize::try_from(u32::from_be_bytes(header)).map_err(|_| WorkerError::FrameTooLarge)?;
    validate_frame_length(length)?;
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

pub(super) fn write_frame(
    writer: &mut impl Write,
    value: &impl Serialize,
) -> Result<(), WorkerError> {
    let frame = encode_frame(value)?;
    writer.write_all(&frame)?;
    writer.flush()?;
    Ok(())
}

fn validate_frame_length(length: usize) -> Result<(), WorkerError> {
    if length == 0 {
        return Err(WorkerError::ZeroLengthFrame);
    }
    if length > MAX_FRAME_BYTES {
        return Err(WorkerError::FrameTooLarge);
    }
    Ok(())
}

pub(super) fn decode_payload<T: DeserializeOwned>(payload: &[u8]) -> Result<T, WorkerError> {
    let mut stream = serde_json::Deserializer::from_slice(payload).into_iter::<T>();
    let value = stream.next().ok_or(WorkerError::ZeroLengthFrame)??;
    if stream.byte_offset() != payload.len() {
        return Err(WorkerError::TrailingBytes);
    }
    Ok(value)
}

pub(super) fn verify_binding(
    name: &'static str,
    supplied: &str,
    expected: &str,
) -> Result<(), WorkerError> {
    if !valid_hash(supplied) || supplied != expected {
        return Err(WorkerError::BindingMismatch(name));
    }
    Ok(())
}
