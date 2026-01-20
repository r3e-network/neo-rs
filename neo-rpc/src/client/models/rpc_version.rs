// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_version.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_json::{JArray, JObject, JToken};
use std::collections::HashMap;

/// RPC version information matching C# RpcVersion
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
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "network".to_string(),
            JToken::Number(self.protocol.network as f64),
        ); // Obsolete
        json.insert("tcpport".to_string(), JToken::Number(self.tcp_port as f64));
        json.insert("nonce".to_string(), JToken::Number(self.nonce as f64));
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
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let tcp_port = json
            .get("tcpport")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'tcpport' field")? as i32;

        let nonce = json
            .get("nonce")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'nonce' field")? as u32;

        let user_agent = json
            .get("useragent")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'useragent' field")?
            .to_string();

        let protocol_json = json
            .get("protocol")
            .and_then(|v| v.as_object())
            .ok_or("Missing or invalid 'protocol' field")?;
        let protocol = RpcProtocol::from_json(protocol_json)?;

        Ok(Self {
            tcp_port,
            nonce,
            user_agent,
            protocol,
        })
    }
}

/// RPC protocol information matching C# RpcProtocol
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

    /// Hardforks
    pub hardforks: HashMap<String, u32>,

    /// Seed list
    pub seed_list: Vec<String>,

    /// Standby committee
    pub standby_committee: Vec<String>,
}

impl RpcProtocol {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("network".to_string(), JToken::Number(self.network as f64));
        json.insert(
            "validatorscount".to_string(),
            JToken::Number(self.validators_count as f64),
        );
        json.insert(
            "msperblock".to_string(),
            JToken::Number(self.milliseconds_per_block as f64),
        );
        json.insert(
            "maxvaliduntilblockincrement".to_string(),
            JToken::Number(self.max_valid_until_block_increment as f64),
        );
        json.insert(
            "maxtraceableblocks".to_string(),
            JToken::Number(self.max_traceable_blocks as f64),
        );
        json.insert(
            "addressversion".to_string(),
            JToken::Number(self.address_version as f64),
        );
        json.insert(
            "maxtransactionsperblock".to_string(),
            JToken::Number(self.max_transactions_per_block as f64),
        );
        json.insert(
            "memorypoolmaxtransactions".to_string(),
            JToken::Number(self.memory_pool_max_transactions as f64),
        );
        json.insert(
            "initialgasdistribution".to_string(),
            JToken::Number(self.initial_gas_distribution as f64),
        );

        // Hardforks array
        let hardforks_array: Vec<JToken> = self
            .hardforks
            .iter()
            .map(|(name, height)| {
                let mut obj = JObject::new();
                obj.insert("name".to_string(), JToken::String(name.clone()));
                obj.insert("blockheight".to_string(), JToken::Number(*height as f64));
                JToken::Object(obj)
            })
            .collect();
        json.insert(
            "hardforks".to_string(),
            JToken::Array(JArray::from(hardforks_array)),
        );

        // Standby committee array
        let committee_array: Vec<JToken> = self
            .standby_committee
            .iter()
            .map(|member| JToken::String(member.clone()))
            .collect();
        json.insert(
            "standbycommittee".to_string(),
            JToken::Array(JArray::from(committee_array)),
        );

        // Seed list array
        let seed_array: Vec<JToken> = self
            .seed_list
            .iter()
            .map(|s| JToken::String(s.clone()))
            .collect();
        json.insert(
            "seedlist".to_string(),
            JToken::Array(JArray::from(seed_array)),
        );

        json
    }

    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let network = json
            .get("network")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'network' field")? as u32;

        let validators_count =
            json.get("validatorscount")
                .and_then(|v| v.as_number())
                .ok_or("Missing or invalid 'validatorscount' field")? as i32;

        let milliseconds_per_block =
            json.get("msperblock")
                .and_then(|v| v.as_number())
                .ok_or("Missing or invalid 'msperblock' field")? as u32;

        let max_valid_until_block_increment = json
            .get("maxvaliduntilblockincrement")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'maxvaliduntilblockincrement' field")?
            as u32;

        let max_traceable_blocks =
            json.get("maxtraceableblocks")
                .and_then(|v| v.as_number())
                .ok_or("Missing or invalid 'maxtraceableblocks' field")? as u32;

        let address_version =
            json.get("addressversion")
                .and_then(|v| v.as_number())
                .ok_or("Missing or invalid 'addressversion' field")? as u8;

        let max_transactions_per_block =
            json.get("maxtransactionsperblock")
                .and_then(|v| v.as_number())
                .ok_or("Missing or invalid 'maxtransactionsperblock' field")? as u32;

        let memory_pool_max_transactions =
            json.get("memorypoolmaxtransactions")
                .and_then(|v| v.as_number())
                .ok_or("Missing or invalid 'memorypoolmaxtransactions' field")? as i32;

        let initial_gas_distribution =
            json.get("initialgasdistribution")
                .and_then(|v| v.as_number())
                .ok_or("Missing or invalid 'initialgasdistribution' field")? as u64;

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
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();

        // Parse seed list
        let seed_list = json
            .get("seedlist")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_string())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        // Parse standby committee
        let standby_committee = json
            .get("standbycommittee")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_string())
                    .collect()
            })
            .unwrap_or_default();

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
mod tests {
    use super::*;
    use neo_json::JToken;
    use std::fs;
    use std::path::PathBuf;

    fn sample_protocol() -> RpcProtocol {
        let mut hardforks = HashMap::new();
        hardforks.insert("neo3".to_string(), 0);

        RpcProtocol {
            network: 5195086,
            validators_count: 7,
            milliseconds_per_block: 15_000,
            max_valid_until_block_increment: 10,
            max_traceable_blocks: 100_000,
            address_version: 53,
            max_transactions_per_block: 512,
            memory_pool_max_transactions: 50_000,
            initial_gas_distribution: 5_200_000_000_000_000,
            hardforks,
            seed_list: vec!["seed1".into(), "seed2".into()],
            standby_committee: vec!["comm1".into(), "comm2".into()],
        }
    }

    #[test]
    fn rpc_version_roundtrip() {
        let version = RpcVersion {
            tcp_port: 10333,
            nonce: 42,
            user_agent: "/NEO:3.6/".into(),
            protocol: sample_protocol(),
        };

        let json = version.to_json();
        let parsed = RpcVersion::from_json(&json).expect("version");

        assert_eq!(parsed.tcp_port, version.tcp_port);
        assert_eq!(parsed.nonce, version.nonce);
        assert_eq!(parsed.user_agent, version.user_agent);
        assert_eq!(parsed.protocol.network, version.protocol.network);
        assert_eq!(
            parsed.protocol.hardforks.get("neo3"),
            version.protocol.hardforks.get("neo3")
        );
        assert_eq!(parsed.protocol.seed_list.len(), 2);
        assert_eq!(parsed.protocol.standby_committee.len(), 2);
    }

    fn load_rpc_case_result(name: &str) -> JObject {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("neo_csharp");
        path.push("node");
        path.push("tests");
        path.push("Neo.Network.RPC.Tests");
        path.push("RpcTestCases.json");
        let payload = fs::read_to_string(&path).expect("read RpcTestCases.json");
        let token = JToken::parse(&payload, 128).expect("parse RpcTestCases.json");
        let cases = token
            .as_array()
            .expect("RpcTestCases.json should be an array");
        for entry in cases.children() {
            let token = entry.as_ref().expect("array entry");
            let obj = token.as_object().expect("case object");
            let case_name = obj
                .get("Name")
                .and_then(|value| value.as_string())
                .unwrap_or_default();
            if case_name.eq_ignore_ascii_case(name) {
                let response = obj
                    .get("Response")
                    .and_then(|value| value.as_object())
                    .expect("case response");
                let result = response
                    .get("result")
                    .and_then(|value| value.as_object())
                    .expect("case result");
                return result.clone();
            }
        }
        panic!("RpcTestCases.json missing case: {name}");
    }

    #[test]
    fn version_to_json_matches_rpc_test_case() {
        let expected = load_rpc_case_result("getversionasync");
        let parsed = RpcVersion::from_json(&expected).expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }
}
