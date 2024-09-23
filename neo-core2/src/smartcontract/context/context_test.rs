use std::collections::HashMap;
use neo_core2::crypto::{keys, hash::Uint160};
use neo_core2::network::NetworkType;
use neo_core2::smartcontract::{ParameterContext, Contract, Parameter, ParameterType};
use neo_core2::transaction::{Transaction, Witness};
use neo_core2::vm::{Opcode, VM};

#[derive(Default)]
struct VerifStub;

impl VerifStub {
    fn hash(&self) -> Uint160 {
        Uint160::from_slice(&[1, 2, 3])
    }
    fn encode_hashable_fields(&self) -> Vec<u8> {
        vec![1]
    }
    fn decode_hashable_fields(&mut self, _data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

#[test]
fn test_parameter_context_add_signature_simple_contract() {
    let priv_key = keys::PrivateKey::new().unwrap();
    let pub_key = priv_key.public_key();
    let tx = get_contract_tx(pub_key.script_hash());
    let sig = priv_key.sign_hashable(NetworkType::UnitTestNet as u32, &tx);

    // Test invalid contract
    {
        let mut c = ParameterContext::new("Neo.Core.ContractTransaction", NetworkType::UnitTestNet, tx.clone());
        let ctr = Contract {
            script: pub_key.get_verification_script(),
            parameters: vec![
                new_param(ParameterType::Signature, "parameter0"),
                new_param(ParameterType::Signature, "parameter1"),
            ],
        };
        assert!(c.add_signature(ctr.script_hash(), &ctr, &pub_key, &sig).is_err());
        if let Some(item) = c.items.get(&ctr.script_hash()) {
            assert!(item.parameters[0].value.is_none());
        }

        let ctr_no_params = Contract {
            script: pub_key.get_verification_script(),
            parameters: vec![],
        };
        assert!(c.add_signature(ctr_no_params.script_hash(), &ctr_no_params, &pub_key, &sig).is_err());
        if let Some(item) = c.items.get(&ctr_no_params.script_hash()) {
            assert!(item.parameters[0].value.is_none());
        }
    }

    let mut c = ParameterContext::new("Neo.Core.ContractTransaction", NetworkType::UnitTestNet, tx);
    let ctr = Contract {
        script: pub_key.get_verification_script(),
        parameters: vec![new_param(ParameterType::Signature, "parameter0")],
    };
    assert!(c.add_signature(ctr.script_hash(), &ctr, &pub_key, &sig).is_ok());
    let item = c.items.get(&ctr.script_hash()).unwrap();
    assert_eq!(item.parameters[0].value, Some(sig.clone()));

    // Test GetWitness
    {
        let w = c.get_witness(ctr.script_hash()).unwrap();
        let mut v = new_test_vm(&w, &tx);
        assert!(v.run().is_ok());
        assert_eq!(v.estack().len(), 1);
        assert_eq!(v.estack().pop().unwrap().as_bool(), true);
    }

    // Test not found
    {
        let ctr = Contract {
            script: vec![Opcode::DROP as u8, Opcode::PUSHT as u8],
            parameters: vec![new_param(ParameterType::Signature, "parameter0")],
        };
        assert!(c.get_witness(ctr.script_hash()).is_err());
    }
}

#[test]
fn test_get_complete_transaction_for_non_tx() {
    let c = ParameterContext::new("Neo.Network.P2P.Payloads.Block", NetworkType::UnitTestNet, VerifStub::default());
    assert!(c.get_complete_transaction().is_err());
}

// Helper functions

fn new_param(typ: ParameterType, name: &str) -> Parameter {
    Parameter {
        name: name.to_string(),
        param_type: typ,
        value: None,
    }
}

fn get_contract_tx(signer: Uint160) -> Transaction {
    let mut tx = Transaction::new(vec![Opcode::PUSH1 as u8], 0);
    tx.attributes = vec![];
    tx.witnesses = vec![];
    tx.signers = vec![signer];
    tx.hash();
    tx
}

fn new_test_vm(w: &Witness, tx: &Transaction) -> VM {
    let mut vm = VM::new(NetworkType::UnitTestNet as u32, tx);
    vm.load_script(&w.verification_script);
    vm.load_script(&w.invocation_script);
    vm
}
