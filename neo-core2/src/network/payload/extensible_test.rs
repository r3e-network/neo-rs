use std::io;
use std::sync::Arc;
use std::sync::Mutex;

use crate::network::payload::Extensible;
use crate::network::payload::MAX_SIZE;
use crate::core::transaction::Witness;
use crate::util::random;
use crate::testserdes;
use anyhow::Result;

#[test]
fn test_extensible_serializable() -> Result<()> {
    let expected = Extensible {
        category: "test".to_string(),
        valid_block_start: 12,
        valid_block_end: 1234,
        sender: random::uint160(),
        data: random::bytes(4),
        witness: Witness {
            invocation_script: random::bytes(3),
            verification_script: random::bytes(3),
        },
    };

    testserdes::encode_decode_binary(&expected)?;

    let mut w = io::BufWriter::new(Vec::new());
    expected.encode_binary_unsigned(&mut w)?;
    let unsigned = w.into_inner()?;

    let result = testserdes::decode_binary::<Extensible>(&unsigned);
    assert!(result.is_err());

    let mut invalid_data = unsigned.clone();
    invalid_data.push(42);
    let result = testserdes::decode_binary::<Extensible>(&invalid_data);
    assert!(result.is_err());

    let mut oversized_data = expected.clone();
    oversized_data.data = vec![0; MAX_SIZE + 1];
    let mut w = io::BufWriter::new(Vec::new());
    oversized_data.encode_binary_unsigned(&mut w)?;
    let unsigned = w.into_inner()?;
    let result = testserdes::decode_binary::<Extensible>(&unsigned);
    assert!(result.is_err());

    Ok(())
}

#[test]
fn test_extensible_hashes() {
    fn get_extensible_pair() -> (Extensible, Extensible) {
        let mut p1 = Extensible::new();
        p1.data = vec![1, 2, 3];
        let mut p2 = Extensible::new();
        p2.data = vec![3, 2, 1];
        (p1, p2)
    }

    let (p1, p2) = get_extensible_pair();
    assert_ne!(p1.hash(), p2.hash());
    assert_ne!(p1.hash(), p2.hash());
}
