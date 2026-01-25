// Converted from C# Neo.UnitTests.UT_UInt160
use crate::{PrimitiveError, UInt160};

#[cfg(test)]
mod uint160_tests {
    use super::*;

    #[test]
    fn test_fail() {
        let result = UInt160::from_bytes(&vec![0u8; UInt160::LENGTH + 1]);
        assert!(result.is_err());
    }

    #[test]
    fn test_generator1() {
        let uint160 = UInt160::new();
        assert_eq!(uint160, UInt160::zero());
    }

    #[test]
    fn test_generator2() {
        let bytes = vec![0u8; 20];
        let uint160 = UInt160::from_bytes(&bytes).unwrap();
        assert_eq!(uint160, UInt160::zero());
    }

    #[test]
    fn test_generator3() {
        let uint160 = UInt160::parse("0xff00000000000000000000000000000000000001").unwrap();
        assert_eq!(
            "0xff00000000000000000000000000000000000001",
            uint160.to_hex_string()
        );
    }

    #[test]
    fn test_generator4() {
        let uint160 = UInt160::parse("0x0102030405060708090a0b0c0d0e0f1011121314").unwrap();
        assert_eq!(
            "0x0102030405060708090a0b0c0d0e0f1011121314",
            uint160.to_hex_string()
        );
    }

    #[test]
    fn test_compare_to() {
        let mut temp = vec![0u8; 20];
        temp[19] = 0x01;
        let result = UInt160::from_bytes(&temp).unwrap();

        assert_eq!(
            std::cmp::Ordering::Equal,
            UInt160::zero().cmp(&UInt160::zero())
        );
        assert_eq!(std::cmp::Ordering::Less, UInt160::zero().cmp(&result));
        assert_eq!(std::cmp::Ordering::Greater, result.cmp(&UInt160::zero()));
        assert_eq!(
            std::cmp::Ordering::Equal,
            result.cmp(&UInt160::from_bytes(&temp).unwrap())
        );
    }

    #[test]
    fn test_equals() {
        let mut temp = vec![0u8; 20];
        temp[19] = 0x01;
        let result = UInt160::from_bytes(&temp).unwrap();

        assert!(UInt160::zero().equals(Some(&UInt160::zero())));
        assert!(!UInt160::zero().equals(Some(&result)));
        assert!(!UInt160::zero().equals(None));
        assert_eq!(UInt160::zero(), UInt160::zero());
        assert_ne!(UInt160::zero(), result);
        assert_eq!(
            UInt160::zero(),
            UInt160::parse("0x0000000000000000000000000000000000000000").unwrap()
        );
        assert_ne!(
            UInt160::zero(),
            UInt160::parse("0x0000000000000000000000000000000000000001").unwrap()
        );
    }

    #[test]
    fn test_parse() {
        // Test null/empty parsing
        assert!(UInt160::parse("").is_err());

        let result = UInt160::parse("0x0000000000000000000000000000000000000000").unwrap();
        assert_eq!(UInt160::zero(), result);

        // Test invalid length
        assert!(UInt160::parse("000000000000000000000000000000000000000").is_err());

        let result1 = UInt160::parse("0000000000000000000000000000000000000000").unwrap();
        assert_eq!(UInt160::zero(), result1);
    }

    #[test]
    fn test_try_parse() {
        let mut result = None;
        assert!(!UInt160::try_parse("", &mut result));
        assert!(result.is_none());

        assert!(UInt160::try_parse(
            "0x0000000000000000000000000000000000000000",
            &mut result
        ));
        assert_eq!(result.unwrap(), UInt160::zero());

        result = None;
        assert!(!UInt160::try_parse(
            "000000000000000000000000000000000000000",
            &mut result
        ));
        assert!(result.is_none());

        assert!(UInt160::try_parse(
            "0000000000000000000000000000000000000000",
            &mut result
        ));
        assert_eq!(result.unwrap(), UInt160::zero());
    }

    #[test]
    fn test_get_hash_code() {
        let a = UInt160::zero();
        let b = UInt160::parse("0x0000000000000000000000000000000000000001").unwrap();

        assert_eq!(a.get_hash_code(), UInt160::zero().get_hash_code());
        assert_ne!(a.get_hash_code(), b.get_hash_code());
    }

    #[test]
    fn test_to_string() {
        let uint160 = UInt160::parse("0x0102030405060708090a0b0c0d0e0f1011121314").unwrap();
        // to_string uses Display which calls to_hex_string, returning the same format as input
        assert_eq!(
            "0x0102030405060708090a0b0c0d0e0f1011121314",
            uint160.to_string()
        );
    }
}
