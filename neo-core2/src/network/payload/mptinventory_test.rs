use crate::testserdes;
use crate::util::Uint256;
use crate::network::payload::{MPTInventory, NewMPTInventory, MaxMPTHashesCount};
use std::error::Error;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testserdes;
    use crate::util::Uint256;
    use crate::network::payload::{MPTInventory, NewMPTInventory, MaxMPTHashesCount};
    use std::error::Error;

    #[test]
    fn test_mptinventory_encode_decode_binary() -> Result<(), Box<dyn Error>> {
        // Empty case
        {
            testserdes::encode_decode_binary(&NewMPTInventory(vec![]), &MPTInventory::default())?;
        }

        // Good case
        {
            let inv = NewMPTInventory(vec![Uint256::from([1, 2, 3]), Uint256::from([2, 3, 4])]);
            testserdes::encode_decode_binary(&inv, &MPTInventory::default())?;
        }

        // Too large case
        {
            let check = |count: usize, fail: bool| -> Result<(), Box<dyn Error>> {
                let mut h = vec![Uint256::default(); count];
                for i in 0..count {
                    h[i] = Uint256::from([1, 2, 3]);
                }
                if fail {
                    let bytes = testserdes::encode_binary(&NewMPTInventory(h.clone()))?;
                    assert!(testserdes::decode_binary(&bytes, &mut MPTInventory::default()).is_err());
                } else {
                    testserdes::encode_decode_binary(&NewMPTInventory(h), &MPTInventory::default())?;
                }
                Ok(())
            };
            check(MaxMPTHashesCount, false)?;
            check(MaxMPTHashesCount + 1, true)?;
        }

        Ok(())
    }
}
