use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::str::FromStr;

use neo_crypto::{VerifiableDecodable, keys::PublicKey};
use neo_types::{Uint160, Uint256};
use neo_vm::{emit, script::Script};
use neo_io::BufBinWriter;
use neo_wallet::Contract;
use neo_transaction::{Transaction, Witness};
use neo_smartcontract::{Parameter, ParameterType};

// Constants
const TRANSACTION_TYPE: &str = "Neo.Network.P2P.Payloads.Transaction";
const COMPAT_TRANSACTION_TYPE: &str = "Neo.Core.ContractTransaction";

// Structs
pub struct ParameterContext {
    r#type: String,
    network: u32, // Assuming netmode::Magic is a u32
    verifiable: Box<dyn VerifiableDecodable>,
    items: HashMap<Uint160, Item>,
}

struct Item {
    script: Option<Vec<u8>>,
    parameters: Vec<Parameter>,
    signatures: HashMap<String, Vec<u8>>,
}

struct SigWithIndex {
    index: usize,
    sig: Vec<u8>,
}

// Implementation
impl ParameterContext {
    pub fn new(typ: String, network: u32, verif: Box<dyn VerifiableDecodable>) -> Self {
        ParameterContext {
            r#type: typ,
            network,
            verifiable: verif,
            items: HashMap::new(),
        }
    }

    pub fn get_complete_transaction(&mut self) -> Result<Transaction, Box<dyn Error>> {
        let tx = self.verifiable.as_any().downcast_ref::<Transaction>()
            .ok_or_else(|| "verifiable item is not a transaction".to_string())?;
        
        let mut new_tx = tx.clone();
        new_tx.scripts.clear();

        for (i, signer) in tx.signers.iter().enumerate() {
            let witness = self.get_witness(&signer.account)
                .map_err(|e| format!("can't create witness for signer #{}: {}", i, e))?;
            new_tx.scripts.push(witness);
        }

        Ok(new_tx)
    }

    pub fn get_witness(&self, h: &Uint160) -> Result<Witness, Box<dyn Error>> {
        let item = self.items.get(h).ok_or("witness not found")?;
        let mut bw = BufBinWriter::new();

        for (i, param) in item.parameters.iter().enumerate() {
            if param.r#type != ParameterType::Signature {
                return Err(format!("unsupported {} parameter #{}", param.r#type, i).into());
            }
            if param.value.is_none() {
                return Err(format!("no value for parameter #{} (not signed yet?)", i).into());
            }
            emit::bytes(&mut bw, param.value.as_ref().unwrap());
        }

        Ok(Witness {
            invocation_script: bw.to_vec(),
            verification_script: item.script.clone().unwrap_or_default(),
        })
    }

    pub fn add_signature(&mut self, h: Uint160, ctr: &Contract, pub_key: &PublicKey, sig: Vec<u8>) -> Result<(), Box<dyn Error>> {
        let item = self.get_item_for_contract(h, ctr);

        if let Some((_, pubs)) = Script::parse_multi_sig_contract(&ctr.script) {
            if item.get_signature(pub_key).is_some() {
                return Err("signature is already added".into());
            }

            let pub_bytes = pub_key.to_bytes();
            if !pubs.iter().any(|p| p == &pub_bytes) {
                return Err("public key is not present in script".into());
            }

            item.add_signature(pub_key, sig);

            if item.signatures.len() >= ctr.parameters.len() {
                let mut index_map = HashMap::new();
                for (i, pub_key) in pubs.iter().enumerate() {
                    index_map.insert(hex::encode(pub_key), i);
                }

                let mut sigs: Vec<SigWithIndex> = item.signatures.iter()
                    .take(item.parameters.len())
                    .map(|(pub_key, sig)| SigWithIndex {
                        index: *index_map.get(pub_key).unwrap(),
                        sig: sig.clone(),
                    })
                    .collect();

                sigs.sort_by_key(|s| s.index);

                for (i, sig) in sigs.iter().enumerate() {
                    item.parameters[i] = Parameter {
                        r#type: ParameterType::Signature,
                        value: Some(sig.sig.clone()),
                    };
                }
            }
        } else {
            let index = ctr.parameters.iter()
                .position(|p| p.r#type == ParameterType::Signature)
                .ok_or("missing signature parameter")?;

            item.parameters[index].value = Some(sig);
        }

        Ok(())
    }

    fn get_item_for_contract(&mut self, h: Uint160, ctr: &Contract) -> &mut Item {
        self.items.entry(ctr.script_hash()).or_insert_with(|| {
            let params = ctr.parameters.iter()
                .map(|p| Parameter {
                    r#type: p.r#type,
                    value: None,
                })
                .collect();

            Item {
                script: if ctr.deployed { None } else { Some(ctr.script.clone()) },
                parameters: params,
                signatures: HashMap::new(),
            }
        })
    }
}

// Implement serialization/deserialization traits (e.g., serde::Serialize, serde::Deserialize) as needed

impl fmt::Display for ParameterContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ParameterContext {{ type: {}, network: {}, ... }}", self.r#type, self.network)
    }
}

impl FromStr for ParameterContext {
    type Err = Box<dyn Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Implement string parsing logic here
        unimplemented!("ParameterContext::from_str not implemented")
    }
}
