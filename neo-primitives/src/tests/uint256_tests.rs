// Converted from C# Neo.UnitTests.UT_UInt256
use crate::UInt256;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fail() {
        let result = UInt256::from_bytes(&[0u8; UInt256::LENGTH + 1]);
        assert!(result.is_err());
    }

    #[test]
    fn test_generator1() {
        let uint256 = UInt256::new();
        assert_eq!(uint256, UInt256::zero());
    }

    #[test]
    fn test_generator2() {
        let bytes = vec![0u8; 32];
        let uint256 = UInt256::from_bytes(&bytes).unwrap();
        assert_eq!(UInt256::zero(), uint256);
    }

    #[test]
    fn test_generator3() {
        let uint256 =
            UInt256::parse("0xff00000000000000000000000000000000000000000000000000000000000001")
                .unwrap();
        assert_eq!(
            "0xff00000000000000000000000000000000000000000000000000000000000001",
            uint256.to_hex_string()
        );

        let value =
            UInt256::parse("0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20")
                .unwrap();
        assert_eq!(
            "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
            value.to_hex_string()
        );
    }

    #[test]
    fn test_compare_to() {
        let mut temp = vec![0u8; 32];
        temp[31] = 0x01;
        let result = UInt256::from_bytes(&temp).unwrap();

        assert_eq!(
            std::cmp::Ordering::Equal,
            UInt256::zero().cmp(&UInt256::zero())
        );
        assert_eq!(std::cmp::Ordering::Less, UInt256::zero().cmp(&result));
        assert_eq!(std::cmp::Ordering::Greater, result.cmp(&UInt256::zero()));
        assert_eq!(
            std::cmp::Ordering::Equal,
            result.cmp(&UInt256::from_bytes(&temp).unwrap())
        );
    }

    #[test]
    fn test_equals() {
        let mut temp = vec![0u8; 32];
        temp[31] = 0x01;
        let result = UInt256::from_bytes(&temp).unwrap();

        assert!(UInt256::zero().equals(Some(&UInt256::zero())));
        assert!(!UInt256::zero().equals(Some(&result)));
        assert!(!result.equals(None));
    }

    #[test]
    fn test_equals1() {
        let temp1 = UInt256::new();
        let temp2 = UInt256::new();
        assert_eq!(temp1, temp2);
    }

    #[test]
    fn test_parse() {
        assert!(UInt256::parse("").is_err());

        let result =
            UInt256::parse("0x0000000000000000000000000000000000000000000000000000000000000000")
                .unwrap();
        assert_eq!(UInt256::zero(), result);

        assert!(
            UInt256::parse("00000000000000000000000000000000000000000000000000000000000000")
                .is_err()
        );

        let result1 =
            UInt256::parse("0000000000000000000000000000000000000000000000000000000000000000")
                .unwrap();
        assert_eq!(UInt256::zero(), result1);
    }

    #[test]
    fn test_try_parse() {
        let mut result = None;
        assert!(!UInt256::try_parse("", &mut result));
        assert!(result.is_none());

        assert!(UInt256::try_parse(
            "0x0000000000000000000000000000000000000000000000000000000000000000",
            &mut result
        ));
        assert_eq!(result.unwrap(), UInt256::zero());

        result = None;
        assert!(!UInt256::try_parse(
            "00000000000000000000000000000000000000000000000000000000000000",
            &mut result
        ));
        assert!(result.is_none());

        assert!(UInt256::try_parse(
            "0000000000000000000000000000000000000000000000000000000000000000",
            &mut result
        ));
        assert_eq!(result.unwrap(), UInt256::zero());
    }

    #[test]
    fn test_get_hash_code() {
        let a = UInt256::zero();
        let b =
            UInt256::parse("0x0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap();

        assert_eq!(a.get_hash_code(), UInt256::zero().get_hash_code());
        assert_ne!(a.get_hash_code(), b.get_hash_code());
    }

    #[test]
    fn test_to_string() {
        let uint256 =
            UInt256::parse("0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20")
                .unwrap();
        // to_string uses Display which calls to_hex_string, returning the same format as input
        assert_eq!(
            "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
            uint256.to_string()
        );
    }
}
