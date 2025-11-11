#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HandshakeState {
    AwaitingRemoteVersion,
    AwaitingRemoteVerack,
    Completed,
}
