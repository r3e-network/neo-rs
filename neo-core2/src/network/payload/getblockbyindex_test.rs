use crate::network::payload::{GetBlockByIndex, MaxHeadersAllowed, NewGetBlockByIndex};
use crate::testserdes;
use anyhow::Result;

#[test]
fn test_get_block_data_encode_decode() -> Result<()> {
    let mut d = NewGetBlockByIndex(123, 100);
    testserdes::encode_decode_binary(&d, &mut GetBlockByIndex::default())?;

    // invalid block count
    d = NewGetBlockByIndex(5, 0);
    let data = testserdes::encode_binary(&d)?;
    assert!(testserdes::decode_binary(&data, &mut GetBlockByIndex::default()).is_err());

    // invalid block count
    d = NewGetBlockByIndex(5, MaxHeadersAllowed + 1);
    let data = testserdes::encode_binary(&d)?;
    assert!(testserdes::decode_binary(&data, &mut GetBlockByIndex::default()).is_err());

    Ok(())
}
