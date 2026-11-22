use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeAction {
    ConsensusSnapshot,
    MempoolSnapshot,
    TransactionPreflight,
}

pub type ProbeRequest = ProbeAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeActionSpec {
    pub action: ProbeAction,
    pub maximum_budget: ProbeBudget,
    pub observation_kind: &'static str,
}

pub const PROBE_ACTION_CATALOG: [ProbeActionSpec; 3] = [
    ProbeActionSpec {
        action: ProbeAction::ConsensusSnapshot,
        maximum_budget: ProbeBudget {
            max_duration_ms: 5_000,
            max_output_bytes: 65_536,
            max_observations: 64,
        },
        observation_kind: "consensus_snapshot",
    },
    ProbeActionSpec {
        action: ProbeAction::MempoolSnapshot,
        maximum_budget: ProbeBudget {
            max_duration_ms: 3_000,
            max_output_bytes: 32_768,
            max_observations: 32,
        },
        observation_kind: "mempool_snapshot",
    },
    ProbeActionSpec {
        action: ProbeAction::TransactionPreflight,
        maximum_budget: ProbeBudget {
            max_duration_ms: 3_000,
            max_output_bytes: 65_536,
            max_observations: 64,
        },
        observation_kind: "transaction_preflight",
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeTransaction {
    pub transaction_hash: String,
    pub transaction_bytes: Vec<u8>,
}

impl ProbeTransaction {
    pub fn validate(&self) -> Result<(), ProbeContractError> {
        if self.transaction_bytes.is_empty()
            || self.transaction_bytes.len() > MAX_PROBE_TRANSACTION_BYTES
            || !valid_hash(&self.transaction_hash)
            || hex_sha256(&self.transaction_bytes) != self.transaction_hash.to_ascii_lowercase()
        {
            return Err(ProbeContractError::InvalidTransaction);
        }
        Ok(())
    }

    pub fn content_hash(&self) -> Result<String, ProbeContractError> {
        self.validate()?;
        hash_serializable(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeContext {
    pub observed_height: u32,
    pub tip_hash: String,
    pub requested_at_unix_ms: u64,
    pub node_instance_id: String,
}

impl ProbeContext {
    pub fn validate(&self) -> Result<(), ProbeContractError> {
        if !valid_hash(&self.tip_hash)
            || self.requested_at_unix_ms == 0
            || self.node_instance_id.is_empty()
            || self.node_instance_id.len() > 128
        {
            return Err(ProbeContractError::InvalidContext);
        }
        Ok(())
    }

    pub fn content_hash(&self) -> Result<String, ProbeContractError> {
        self.validate()?;
        hash_serializable(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeBuild {
    pub binary_hash: String,
    pub version: String,
}

impl ProbeBuild {
    pub fn validate(&self) -> Result<(), ProbeContractError> {
        if !valid_hash(&self.binary_hash)
            || self.version.trim().is_empty()
            || self.version.len() > 128
        {
            return Err(ProbeContractError::InvalidBuild);
        }
        Ok(())
    }

    pub fn content_hash(&self) -> Result<String, ProbeContractError> {
        self.validate()?;
        hash_serializable(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeBudget {
    pub max_duration_ms: u64,
    pub max_output_bytes: u64,
    pub max_observations: u16,
}

impl ProbeBudget {
    pub fn validate_for(self, action: ProbeAction) -> Result<(), ProbeContractError> {
        let maximum = PROBE_ACTION_CATALOG
            .iter()
            .find(|entry| entry.action == action)
            .expect("complete probe action catalog")
            .maximum_budget;
        if self.max_duration_ms == 0
            || self.max_output_bytes == 0
            || self.max_observations == 0
            || self.max_duration_ms > maximum.max_duration_ms
            || self.max_output_bytes > maximum.max_output_bytes
            || self.max_observations > maximum.max_observations
            || self.max_duration_ms > MAX_PROBE_DURATION_MS
            || self.max_output_bytes > MAX_PROBE_OUTPUT_BYTES
            || self.max_observations > MAX_PROBE_OBSERVATIONS
        {
            return Err(ProbeContractError::BudgetExceeded);
        }
        Ok(())
    }

    pub fn content_hash(&self) -> Result<String, ProbeContractError> {
        if self.max_duration_ms == 0 || self.max_output_bytes == 0 || self.max_observations == 0 {
            return Err(ProbeContractError::BudgetExceeded);
        }
        hash_serializable(self)
    }
}
