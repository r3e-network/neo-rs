use alloc::vec::Vec;

use neo_base::encoding::{NeoDecode, NeoEncode, SliceReader};

use crate::tx::{
    NotaryAssisted, OracleCode, OracleResponse, Signer, Tx, TxAttr, Witness, WitnessScope,
    WitnessScopes,
};
use crate::{h160::H160, script::Script};

fn sample_signer() -> Signer {
    let mut scopes = WitnessScopes::default();
    scopes.add_scope(WitnessScope::CalledByEntry);
    Signer {
        account: H160::default(),
        scopes,
        allowed_contract: Vec::new(),
        allowed_groups: Vec::new(),
        rules: Vec::new(),
    }
}

fn sample_tx() -> Tx {
    Tx {
        version: 0,
        nonce: 42,
        valid_until_block: 1000,
        sysfee: 300,
        netfee: 200,
        signers: vec![sample_signer()],
        attributes: vec![
            TxAttr::HighPriority,
            TxAttr::OracleResponse(OracleResponse {
                id: 7,
                code: OracleCode::Success,
                result: neo_base::bytes::Bytes::from(vec![0xAA, 0xBB]),
            }),
            TxAttr::NotaryAssisted(NotaryAssisted { nkeys: 3 }),
        ],
        script: Script::new(vec![0x51]),
        witnesses: vec![Witness::new(
            Script::new(vec![0x51]),
            Script::new(vec![0xAC]),
        )],
    }
}

#[test]
fn tx_roundtrip_preserves_attributes() {
    let tx = sample_tx();
    let mut buf = Vec::new();
    tx.neo_encode(&mut buf);

    let mut reader = SliceReader::new(&buf);
    let decoded = Tx::neo_decode(&mut reader).expect("decode tx");

    assert_eq!(decoded.version, tx.version);
    assert_eq!(decoded.nonce, tx.nonce);
    assert_eq!(decoded.signers.len(), 1);
    assert_eq!(decoded.attributes.len(), tx.attributes.len());
    assert!(matches!(
        decoded.attributes.last().unwrap(),
        TxAttr::NotaryAssisted(NotaryAssisted { nkeys: 3 })
    ));
    assert_eq!(decoded.hash(), tx.hash());
}
