use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::config::{self, netmode::Magic};
use crate::crypto::keys::{self, PublicKeys};
use crate::encoding::fixedn::Fixed8;

#[derive(Serialize, Deserialize)]
pub struct Version {
    #[serde(rename = "tcpport")]
    tcp_port: u16,
    #[serde(rename = "wsport", skip_serializing_if = "Option::is_none")]
    ws_port: Option<u16>,
    #[serde(rename = "nonce")]
    nonce: u32,
    #[serde(rename = "useragent")]
    user_agent: String,
    #[serde(rename = "protocol")]
    protocol: Protocol,
    #[serde(rename = "rpc")]
    rpc: RPC,
}

#[derive(Serialize, Deserialize)]
pub struct RPC {
    #[serde(rename = "maxiteratorresultitems")]
    max_iterator_result_items: i32,
    #[serde(rename = "sessionenabled")]
    session_enabled: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Protocol {
    address_version: u8,
    network: Magic,
    milliseconds_per_block: i32,
    max_traceable_blocks: u32,
    max_valid_until_block_increment: u32,
    max_transactions_per_block: u16,
    memory_pool_max_transactions: i32,
    validators_count: u8,
    initial_gas_distribution: Fixed8,
    hardforks: HashMap<config::Hardfork, u32>,
    standby_committee: PublicKeys,
    seed_list: Vec<String>,
    committee_history: Option<HashMap<u32, u32>>,
    p2p_sig_extensions: Option<bool>,
    state_root_in_header: Option<bool>,
    validators_history: Option<HashMap<u32, u32>>,
}

#[derive(Serialize, Deserialize)]
struct ProtocolMarshallerAux {
    #[serde(rename = "addressversion")]
    address_version: u8,
    #[serde(rename = "network")]
    network: Magic,
    #[serde(rename = "msperblock")]
    milliseconds_per_block: i32,
    #[serde(rename = "maxtraceableblocks")]
    max_traceable_blocks: u32,
    #[serde(rename = "maxvaliduntilblockincrement")]
    max_valid_until_block_increment: u32,
    #[serde(rename = "maxtransactionsperblock")]
    max_transactions_per_block: u16,
    #[serde(rename = "memorypoolmaxtransactions")]
    memory_pool_max_transactions: i32,
    #[serde(rename = "validatorscount")]
    validators_count: u8,
    #[serde(rename = "initialgasdistribution")]
    initial_gas_distribution: i64,
    #[serde(rename = "hardforks")]
    hardforks: Vec<HardforkAux>,
    #[serde(rename = "standbycommittee")]
    standby_committee: Vec<String>,
    #[serde(rename = "seedlist")]
    seed_list: Vec<String>,
    #[serde(rename = "committeehistory", skip_serializing_if = "Option::is_none")]
    committee_history: Option<HashMap<u32, u32>>,
    #[serde(rename = "p2psigextensions", skip_serializing_if = "Option::is_none")]
    p2p_sig_extensions: Option<bool>,
    #[serde(rename = "staterootinheader", skip_serializing_if = "Option::is_none")]
    state_root_in_header: Option<bool>,
    #[serde(rename = "validatorshistory", skip_serializing_if = "Option::is_none")]
    validators_history: Option<HashMap<u32, u32>>,
}

#[derive(Serialize, Deserialize)]
struct HardforkAux {
    name: String,
    #[serde(rename = "blockheight")]
    height: u32,
}

const PREFIX_HARDFORK: &str = "HF_";

impl Protocol {
    pub fn marshal_json(&self) -> Result<String, serde_json::Error> {
        let mut hfs: Vec<HardforkAux> = Vec::with_capacity(self.hardforks.len());
        for hf in &config::HARDFORKS {
            if let Some(&height) = self.hardforks.get(hf) {
                hfs.push(HardforkAux {
                    name: hf.to_string(),
                    height,
                });
            }
        }
        let standby_committee: Vec<String> = self.standby_committee.iter().map(|key| key.to_string_compressed()).collect();

        let aux = ProtocolMarshallerAux {
            address_version: self.address_version,
            network: self.network,
            milliseconds_per_block: self.milliseconds_per_block,
            max_traceable_blocks: self.max_traceable_blocks,
            max_valid_until_block_increment: self.max_valid_until_block_increment,
            max_transactions_per_block: self.max_transactions_per_block,
            memory_pool_max_transactions: self.memory_pool_max_transactions,
            validators_count: self.validators_count,
            initial_gas_distribution: self.initial_gas_distribution.into(),
            hardforks: hfs,
            standby_committee,
            seed_list: self.seed_list.clone(),
            committee_history: self.committee_history.clone(),
            p2p_sig_extensions: self.p2p_sig_extensions,
            state_root_in_header: self.state_root_in_header,
            validators_history: self.validators_history.clone(),
        };
        serde_json::to_string(&aux)
    }

    pub fn unmarshal_json(data: &str) -> Result<Self, serde_json::Error> {
        let aux: ProtocolMarshallerAux = serde_json::from_str(data)?;
        let standby_committee = keys::PublicKeys::new_from_strings(&aux.standby_committee)?;

        let mut hardforks = HashMap::new();
        for hf in aux.hardforks {
            let name = hf.name.trim_start_matches(PREFIX_HARDFORK).to_string();
            if config::is_hardfork_valid(&name) {
                hardforks.insert(config::Hardfork::from_str(&name)?, hf.height);
            } else {
                return Err(serde_json::Error::custom(format!("unexpected hardfork: {}", name)));
            }
        }

        Ok(Protocol {
            address_version: aux.address_version,
            network: aux.network,
            milliseconds_per_block: aux.milliseconds_per_block,
            max_traceable_blocks: aux.max_traceable_blocks,
            max_valid_until_block_increment: aux.max_valid_until_block_increment,
            max_transactions_per_block: aux.max_transactions_per_block,
            memory_pool_max_transactions: aux.memory_pool_max_transactions,
            validators_count: aux.validators_count,
            initial_gas_distribution: Fixed8::from(aux.initial_gas_distribution),
            hardforks,
            standby_committee,
            seed_list: aux.seed_list,
            committee_history: aux.committee_history,
            p2p_sig_extensions: aux.p2p_sig_extensions,
            state_root_in_header: aux.state_root_in_header,
            validators_history: aux.validators_history,
        })
    }
}
