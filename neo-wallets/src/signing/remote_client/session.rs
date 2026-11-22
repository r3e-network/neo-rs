use super::*;
#[derive(Clone)]
pub(super) struct ConfiguredKey {
    pub(super) public_key: Vec<u8>,
    pub(super) account: String,
}

#[derive(Clone)]
pub(super) struct AttestedSession {
    pub(super) session_id: String,
    pub(super) session_public_key: Vec<u8>,
    pub(super) service_identity: String,
    pub(super) policy_version: String,
    pub(super) key_ref: KeyRef,
    pub(super) expires_at_unix_ms: i64,
}

impl AttestedSession {
    pub(super) fn is_valid_for(
        &self,
        now: i64,
        service_identity: &str,
        policy_version: &str,
        key_ref: &KeyRef,
    ) -> bool {
        now < self.expires_at_unix_ms
            && self.service_identity == service_identity
            && self.policy_version == policy_version
            && &self.key_ref == key_ref
    }
}
pub(super) struct RemoteAttestationRuntime {
    pub(super) policy: RemoteAttestationPolicySection,
    pub(super) verifier: Arc<dyn RemoteAttestationVerifier>,
}
