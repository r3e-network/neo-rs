use std::collections::HashSet;
use NeoRust::builder::ScriptBuilder;
use crate::cryptography::ECPoint;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::contract_parameter_type::ContractParameterType;
use crate::uint160::UInt160;

pub trait Contract {
    fn script(&self) -> &Vec<u8>;
    fn parameter_list(&self) -> &Vec<ContractParameterType>;
    fn script_hash(&mut self) -> UInt160;
}

#[derive(Clone, Debug)]
pub struct StandardContract {
    /// The script of the contract.
    pub script: Vec<u8>,

    /// The parameters of the contract.
    pub parameter_list: Vec<ContractParameterType>,

    script_hash: Option<UInt160>,
}

impl StandardContract {
    /// Creates a new instance of the StandardContract.
    pub fn new(parameter_list: Vec<ContractParameterType>, redeem_script: Vec<u8>) -> Self {
        Self {
            script: redeem_script,
            parameter_list,
            script_hash: None,
        }
    }

    /// Constructs a special contract with empty script, will get the script with script_hash from blockchain when doing the verification.
    pub fn with_hash(script_hash: UInt160, parameter_list: Vec<ContractParameterType>) -> Self {
        Self {
            script: Vec::new(),
            parameter_list,
            script_hash: Some(script_hash),
        }
    }

    /// Creates a multi-sig contract.
    pub fn multi_sig(m: usize, public_keys: &[ECPoint]) -> Self {
        Self {
            script: Self::create_multi_sig_redeem_script(m, public_keys),
            parameter_list: vec![ContractParameterType::Signature; m],
            script_hash: None,
        }
    }

    /// Creates the script of multi-sig contract.
    pub fn create_multi_sig_redeem_script(m: usize, public_keys: &[ECPoint]) -> Vec<u8> {
        if !(1 <= m && m <= public_keys.len() && public_keys.len() <= 1024) {
            panic!("Invalid arguments for multi-sig contract");
        }
        let mut sb = ScriptBuilder::new();
        sb.emit_push(m as i32);
        for public_key in public_keys.iter().cloned().collect::<HashSet<_>>().into_iter() {
            sb.emit_push(&public_key.encode_point(true));
        }
        sb.emit_push(public_keys.len() as i32);
        sb.emit_sys_call(ApplicationEngine::System_Crypto_CheckMultisig);
        sb.to_array()
    }

    /// Creates a signature contract.
    pub fn signature(public_key: &ECPoint) -> Self {
        Self {
            script: Self::create_signature_redeem_script(public_key),
            parameter_list: vec![ContractParameterType::Signature],
            script_hash: None,
        }
    }

    /// Creates the script of signature contract.
    pub fn create_signature_redeem_script(public_key: &ECPoint) -> Vec<u8> {
        let mut sb = ScriptBuilder::new();
        sb.emit_push(&public_key.encode_point(true));
        sb.emit_sys_call(ApplicationEngine::System_Crypto_CheckSig);
        sb.to_array()
    }

    /// Gets the BFT address for the specified public keys.
    pub fn get_bft_address(pubkeys: &[ECPoint]) -> UInt160 {
        let m = pubkeys.len() - (pubkeys.len() - 1) / 3;
        UInt160::from_script(&Self::create_multi_sig_redeem_script(m, pubkeys))
    }
}

impl Contract for StandardContract {
    fn script(&self) -> &Vec<u8> {
        &self.script
    }

    fn parameter_list(&self) -> &Vec<ContractParameterType> {
        &self.parameter_list
    }

    fn script_hash(&mut self) -> UInt160 {
        if let Some(hash) = self.script_hash {
            hash
        } else {
            let hash = UInt160::from_script(&self.script);
            self.script_hash = Some(hash);
            hash
        }
    }
}
