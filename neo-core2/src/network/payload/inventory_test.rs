use std::str;
use std::string::String;
use std::vec::Vec;
use crate::network::payload::{Inventory, InventoryType, BlockType, TXType, ExtensibleType, P2PNotaryRequestType};
use crate::crypto::hash;
use crate::util;
use crate::testserdes;
use assert;
use require;

#[test]
fn test_inventory_encode_decode() {
    let hashes = vec![
        hash::sha256(b"a"),
        hash::sha256(b"b"),
    ];
    let inv = Inventory::new(BlockType, hashes);

    testserdes::encode_decode_binary(&inv, Inventory::default());
}

#[test]
fn test_empty_inv() {
    let msg_inv = Inventory::new(TXType, vec![]);

    let (data, err) = testserdes::encode_binary(&msg_inv);
    assert!(err.is_none());
    assert_eq!(data, vec![TXType as u8, 0]);
    assert_eq!(msg_inv.hashes.len(), 0);
}

#[test]
fn test_valid() {
    require!(TXType.valid(false));
    require!(TXType.valid(true));
    require!(BlockType.valid(false));
    require!(BlockType.valid(true));
    require!(ExtensibleType.valid(false));
    require!(ExtensibleType.valid(true));
    require!(!P2PNotaryRequestType.valid(false));
    require!(P2PNotaryRequestType.valid(true));
    require!(!InventoryType(0xFF).valid(false));
    require!(!InventoryType(0xFF).valid(true));
}

#[test]
fn test_string() {
    require_eq!(TXType.to_string(), "TX");
    require_eq!(BlockType.to_string(), "block");
    require_eq!(ExtensibleType.to_string(), "extensible");
    require_eq!(P2PNotaryRequestType.to_string(), "p2pNotaryRequest");
    require!(InventoryType(0xFF).to_string().contains("unknown"));
}
