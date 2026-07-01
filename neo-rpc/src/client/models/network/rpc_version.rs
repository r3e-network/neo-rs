use super::super::utility::{object_array_from_iter, parse_string_array_lossy, token_array};
use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
use std::collections::BTreeMap;

/// RPC version information matching C# `RpcVersion`
#[derive(Debug, Clone)]
pub struct RpcVersion {
    /// TCP port
    pub tcp_port: i32,

    /// Nonce
    pub nonce: u32,

    /// User agent string
    pub user_agent: String,

    /// Protocol information
    pub protocol: RpcProtocol,
}

impl RpcVersion {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "network".to_string(),
            JToken::Number(f64::from(self.protocol.network)),
        ); // Obsolete
        json.insert(
            "tcpport".to_string(),
            JToken::Number(f64::from(self.tcp_port)),
        );
        json.insert("nonce".to_string(), JToken::Number(f64::from(self.nonce)));
        json.insert(
            "useragent".to_string(),
            JToken::String(self.user_agent.clone()),
        );
        json.insert(
            "protocol".to_string(),
            JToken::Object(self.protocol.to_json()),
        );
        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let tcp_port = json
            .get("tcpport")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'tcpport' field"))?
            as i32;

        let nonce = json
            .get("nonce")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'nonce' field"))?
            as u32;

        let user_agent = json
            .get("useragent")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'useragent' field"))?;

        let protocol_json = json
            .get("protocol")
            .and_then(|v| v.as_object())
            .ok_or_else(|| CoreError::other("Missing or invalid 'protocol' field"))?;
        let protocol = RpcProtocol::from_json(protocol_json)?;

        Ok(Self {
            tcp_port,
            nonce,
            user_agent,
            protocol,
        })
    }
}

/// RPC protocol information matching C# `RpcProtocol`
#[derive(Debug, Clone)]
pub struct RpcProtocol {
    /// Network ID
    pub network: u32,

    /// Number of validators
    pub validators_count: i32,

    /// Milliseconds per block
    pub milliseconds_per_block: u32,

    /// Max valid until block increment
    pub max_valid_until_block_increment: u32,

    /// Max traceable blocks
    pub max_traceable_blocks: u32,

    /// Address version
    pub address_version: u8,

    /// Max transactions per block
    pub max_transactions_per_block: u32,

    /// Memory pool max transactions
    pub memory_pool_max_transactions: i32,

    /// Initial gas distribution
    pub initial_gas_distribution: u64,

    /// Hardforks (BTreeMap for deterministic JSON serialization order)
    pub hardforks: BTreeMap<String, u32>,

    /// Seed list
    pub seed_list: Vec<String>,

    /// Standby committee
    pub standby_committee: Vec<String>,
}

impl RpcProtocol {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "network".to_string(),
            JToken::Number(f64::from(self.network)),
        );
        json.insert(
            "validatorscount".to_string(),
            JToken::Number(f64::from(self.validators_count)),
        );
        json.insert(
            "msperblock".to_string(),
            JToken::Number(f64::from(self.milliseconds_per_block)),
        );
        json.insert(
            "maxvaliduntilblockincrement".to_string(),
            JToken::Number(f64::from(self.max_valid_until_block_increment)),
        );
        json.insert(
            "maxtraceableblocks".to_string(),
            JToken::Number(f64::from(self.max_traceable_blocks)),
        );
        json.insert(
            "addressversion".to_string(),
            JToken::Number(f64::from(self.address_version)),
        );
        json.insert(
            "maxtransactionsperblock".to_string(),
            JToken::Number(f64::from(self.max_transactions_per_block)),
        );
        json.insert(
            "memorypoolmaxtransactions".to_string(),
            JToken::Number(f64::from(self.memory_pool_max_transactions)),
        );
        json.insert(
            "initialgasdistribution".to_string(),
            JToken::Number(self.initial_gas_distribution as f64),
        );

        json.insert(
            "hardforks".to_string(),
            object_array_from_iter(self.hardforks.iter().map(|(name, height)| {
                let mut obj = JObject::new();
                obj.insert("name".to_string(), JToken::String(name.clone()));
                obj.insert(
                    "blockheight".to_string(),
                    JToken::Number(f64::from(*height)),
                );
                obj
            })),
        );

        json.insert(
            "standbycommittee".to_string(),
            token_array(&self.standby_committee, |member| {
                JToken::String(member.clone())
            }),
        );

        json.insert(
            "seedlist".to_string(),
            token_array(&self.seed_list, |seed| JToken::String(seed.clone())),
        );

        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let network = json
            .get("network")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'network' field"))?
            as u32;

        let validators_count = json
            .get("validatorscount")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'validatorscount' field"))?
            as i32;

        let milliseconds_per_block = json
            .get("msperblock")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'msperblock' field"))?
            as u32;

        let max_valid_until_block_increment = json
            .get("maxvaliduntilblockincrement")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| {
                CoreError::other("Missing or invalid 'maxvaliduntilblockincrement' field")
            })? as u32;

        let max_traceable_blocks = json
            .get("maxtraceableblocks")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'maxtraceableblocks' field"))?
            as u32;

        let address_version = json
            .get("addressversion")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'addressversion' field"))?
            as u8;

        let max_transactions_per_block = json
            .get("maxtransactionsperblock")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'maxtransactionsperblock' field"))?
            as u32;

        let memory_pool_max_transactions = json
            .get("memorypoolmaxtransactions")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| {
                CoreError::other("Missing or invalid 'memorypoolmaxtransactions' field")
            })? as i32;

        let initial_gas_distribution = json
            .get("initialgasdistribution")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'initialgasdistribution' field"))?
            as u64;

        // Parse hardforks
        let hardforks = json
            .get("hardforks")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_object())
                    .filter_map(|obj| {
                        let name = obj.get("name")?.as_string()?;
                        let block_height = obj.get("blockheight")?.as_number()? as u32;
                        Some((name, block_height))
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();

        // Parse seed list
        let seed_list = parse_string_array_lossy(json, "seedlist");

        // Parse standby committee
        let standby_committee = parse_string_array_lossy(json, "standbycommittee");

        Ok(Self {
            network,
            validators_count,
            milliseconds_per_block,
            max_valid_until_block_increment,
            max_traceable_blocks,
            address_version,
            max_transactions_per_block,
            memory_pool_max_transactions,
            initial_gas_distribution,
            hardforks,
            seed_list,
            standby_committee,
        })
    }
}

#[cfg(test)]
#[path = "../../../tests/client/models/network/rpc_version.rs"]
mod tests;
