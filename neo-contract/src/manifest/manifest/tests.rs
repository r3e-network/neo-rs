use alloc::{collections::BTreeMap, vec};
use core::str::FromStr;

use neo_base::{
    encoding::{NeoDecode, NeoEncode},
    hash::Hash160,
    SliceReader,
};
use neo_crypto::{
    ecc256::{Keypair, PrivateKey},
    Secp256r1Sign,
};

use crate::{
    manifest::{
        ContractAbi, ContractFeatures, ContractGroup, ContractManifest, ContractMethod,
        ContractPermission, WildcardContainer,
    },
    ContractParameter, ParameterKind,
};

#[test]
fn roundtrip_manifest() {
    let abi = ContractAbi {
        methods: vec![ContractMethod {
            name: "balanceOf".into(),
            parameters: vec![ContractParameter {
                name: "account".into(),
                kind: ParameterKind::Hash160,
            }],
            return_type: ParameterKind::Integer,
            offset: 0,
            safe: true,
        }],
        events: vec![],
    };

    let manifest = ContractManifest {
        name: "policy".into(),
        groups: Vec::new(),
        features: ContractFeatures {
            storage: true,
            payable: false,
        },
        supported_standards: vec!["NEP-17".into()],
        abi,
        permissions: vec![ContractPermission::allow_all()],
        trusts: WildcardContainer::wildcard(),
        extra: BTreeMap::new(),
    };

    let mut buf = Vec::new();
    manifest.neo_encode(&mut buf);
    let mut reader = SliceReader::new(buf.as_slice());
    let decoded = ContractManifest::neo_decode(&mut reader).unwrap();
    assert_eq!(decoded.name, "policy");
    assert_eq!(decoded.supported_standards, vec!["NEP-17"]);
}

#[test]
fn verify_group_signature() {
    let sk = [
        0x1fu8, 0x23, 0x5a, 0x91, 0xde, 0x07, 0x4c, 0x88, 0xa9, 0xee, 0x10, 0x22, 0xfc, 0x77, 0x35,
        0x42, 0x06, 0xa7, 0xa5, 0x4b, 0x11, 0x9d, 0x91, 0x2b, 0x01, 0x34, 0x8b, 0x20, 0x9a, 0x9c,
        0x77, 0x01,
    ];
    let private = PrivateKey::from_slice(&sk).unwrap();
    let keypair = Keypair::from_private(private).unwrap();
    let contract_hash = Hash160::from_str("0x0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f").unwrap();
    let signature = keypair
        .private_key
        .secp256r1_sign(contract_hash.as_slice())
        .unwrap();
    let group = ContractGroup::new(keypair.public_key.clone(), signature);
    assert!(group.verify_contract(&contract_hash));
}
