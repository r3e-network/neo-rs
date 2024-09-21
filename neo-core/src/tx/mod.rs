// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

pub mod attr;
pub mod signer;
pub mod witness;

#[cfg(any(feature = "std", test))]
pub mod pool;

#[cfg(any(feature = "std", test))]
pub mod pool_event;

#[cfg(test)]
mod pool_test;

use alloc::vec::Vec;

use neo_base::encoding::bin::*;
use serde::{Deserialize, Serialize};
pub use {attr::*, signer::*, witness::*};
#[cfg(any(feature = "std", test))]
pub use {pool::*, pool_event::*};

use crate::types::{Script, VmState, H160, H256};

#[derive(Debug, Clone, Deserialize, Serialize, BinEncode, InnerBinDecode)]
pub struct Tx {
    /// i.e. tx-id, None means no set. Set it to None if hash-fields changed
    #[bin(ignore)]
    #[serde(skip_serializing_if = "Option::is_none", skip_deserializing)]
    hash: Option<H256>,

    /// None means not-computed. Set it to None if hash-fields changed
    #[bin(ignore)]
    #[serde(skip_serializing_if = "Option::is_none", skip_deserializing)]
    size: Option<u32>,

    pub version: u8,
    pub nonce: u32,

    pub sysfee: u64,
    pub netfee: u64,

    #[serde(rename = "validuntilblock")]
    pub valid_until_block: u32,

    pub signers: Vec<Signer>,

    pub attributes: Vec<TxAttr>,

    pub script: Script,

    /// i.e. scripts
    pub witnesses: Vec<Witness>,
}

impl EncodeHashFields for Tx {
    fn encode_hash_fields(&self, w: &mut impl BinWriter) {
        self.version.encode_bin(w); // 1
        self.nonce.encode_bin(w); // 4
        self.sysfee.encode_bin(w); // 8
        self.netfee.encode_bin(w); // 8
        self.valid_until_block.encode_bin(w); // 4
        self.signers.encode_bin(w);
        self.attributes.encode_bin(w);
        self.script.encode_bin(w);
    }
}

impl BinDecoder for Tx {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let mut tx = Self::decode_bin_inner(r)?;
        tx.calc_hash_and_size();
        Ok(tx)
    }
}

impl Tx {
    /// i.e. TxID
    pub fn hash(&self) -> H256 { self.hash.unwrap_or_else(|| self.calc_hash()) }

    pub fn size(&self) -> u32 { self.size.unwrap_or_else(|| self.bin_size() as u32) }

    pub fn fee(&self) -> u64 { self.sysfee + self.netfee }

    pub fn netfee_per_byte(&self) -> u64 { self.netfee / self.size() as u64 }

    pub fn signers(&self) -> Vec<&H160> { self.signers.iter().map(|s| &s.account).collect() }

    pub fn has_signer(&self, signer: &H160) -> bool {
        self.signers.iter().find(|s| s.account.eq(signer)).is_some()
    }

    pub fn conflicts(&self) -> Vec<Conflicts> {
        self.attributes
            .iter()
            .map_while(|attr| match attr {
                TxAttr::Conflicts(conflicts) => Some(conflicts.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn calc_hash_and_size(&mut self) {
        self.size = Some(self.bin_size() as u32); // assume never exceed u32::MAX
        self.hash = Some(self.calc_hash());
    }

    fn calc_hash(&self) -> H256 { self.hash_fields_sha256().into() }
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct StatedTx {
    pub block_index: u32,
    pub tx: Tx,
    pub state: VmState,
}

impl StatedTx {
    #[inline]
    pub fn hash(&self) -> H256 { self.tx.hash() }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_tx_encoding() {
        let mut tx = Tx {
            hash: None,
            size: None,
            version: 1,
            nonce: 0x11223344,
            sysfee: 2233,
            netfee: 4455,
            valid_until_block: 9999,
            signers: Vec::new(),
            attributes: Vec::new(),
            script: Default::default(),
            witnesses: Vec::new(),
        };

        let decode = serde_json::to_string(&tx).expect("json encode should be ok");

        let got: Tx = serde_json::from_str(&decode).expect("json decode should be ok");

        assert!(tx.hash.is_none());
        assert!(tx.size.is_none());
        assert_eq!(got.version, tx.version);
        assert_eq!(got.nonce, tx.nonce);
        assert_eq!(got.sysfee, tx.sysfee);
        assert_eq!(got.netfee, tx.netfee);
        assert_eq!(got.valid_until_block, tx.valid_until_block);

        tx.calc_hash_and_size();
        let decode = serde_json::to_string(&tx).expect("json encode should be ok");

        let mut got: Tx = serde_json::from_str(&decode).expect("json decode should be ok");

        assert_eq!(got.hash, None);
        assert_eq!(got.size, None);

        got.calc_hash_and_size();
        assert_eq!(got.hash, tx.hash);
        assert_eq!(got.size, tx.size);
    }
}
