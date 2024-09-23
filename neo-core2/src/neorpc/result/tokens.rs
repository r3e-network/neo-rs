use serde::{Deserialize, Serialize};
use crate::util::{Uint160, Uint256};

// NEP11Balances is a result for the getnep11balances RPC call.
#[derive(Serialize, Deserialize, Debug)]
pub struct NEP11Balances {
    pub balances: Vec<NEP11AssetBalance>,
    pub address: String,
}

// NEP11Balance is a structure holding balance of a NEP-11 asset.
#[derive(Serialize, Deserialize, Debug)]
pub struct NEP11AssetBalance {
    pub asset: Uint160,
    pub decimals: u8,
    pub name: String,
    pub symbol: String,
    pub tokens: Vec<NEP11TokenBalance>,
}

// NEP11TokenBalance represents balance of a single NFT.
#[derive(Serialize, Deserialize, Debug)]
pub struct NEP11TokenBalance {
    pub id: String,
    pub amount: String,
    pub last_updated: u32,
}

// NEP17Balances is a result for the getnep17balances RPC call.
#[derive(Serialize, Deserialize, Debug)]
pub struct NEP17Balances {
    pub balances: Vec<NEP17Balance>,
    pub address: String,
}

// NEP17Balance represents balance for the single token contract.
#[derive(Serialize, Deserialize, Debug)]
pub struct NEP17Balance {
    pub asset: Uint160,
    pub amount: String,
    pub decimals: u8,
    pub last_updated: u32,
    pub name: String,
    pub symbol: String,
}

// NEP11Transfers is a result for the getnep11transfers RPC.
#[derive(Serialize, Deserialize, Debug)]
pub struct NEP11Transfers {
    pub sent: Vec<NEP11Transfer>,
    pub received: Vec<NEP11Transfer>,
    pub address: String,
}

// NEP11Transfer represents single NEP-11 transfer event.
#[derive(Serialize, Deserialize, Debug)]
pub struct NEP11Transfer {
    pub timestamp: u64,
    pub asset: Uint160,
    pub address: Option<String>,
    pub id: String,
    pub amount: String,
    pub index: u32,
    pub notify_index: u32,
    pub tx_hash: Uint256,
}

// NEP17Transfers is a result for the getnep17transfers RPC.
#[derive(Serialize, Deserialize, Debug)]
pub struct NEP17Transfers {
    pub sent: Vec<NEP17Transfer>,
    pub received: Vec<NEP17Transfer>,
    pub address: String,
}

// NEP17Transfer represents single NEP17 transfer event.
#[derive(Serialize, Deserialize, Debug)]
pub struct NEP17Transfer {
    pub timestamp: u64,
    pub asset: Uint160,
    pub address: Option<String>,
    pub amount: String,
    pub index: u32,
    pub notify_index: u32,
    pub tx_hash: Uint256,
}

// KnownNEP11Properties contains a list of well-known NEP-11 token property names.
pub static KNOWN_NEP11_PROPERTIES: phf::Map<&'static str, bool> = phf::phf_map! {
    "description" => true,
    "image" => true,
    "name" => true,
    "tokenURI" => true,
};
