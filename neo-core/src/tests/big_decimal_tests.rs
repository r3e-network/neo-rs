// Converted from C# Neo.UnitTests.UT_BigDecimal
use crate::big_decimal::BigDecimal;
use num_bigint::BigInt;

#[cfg(test)]
#[allow(clippy::module_inception)]
mod big_decimal_tests {
    use super::*;

    #[test]
    fn test_change_decimals() {
        let original_value = BigDecimal::new(BigInt::from(12300), 5);

        let result1 = original_value.change_decimals(7).unwrap();
        assert_eq!(&BigInt::from(1230000), result1.value());
        assert_eq!(7, result1.decimals());

        let result2 = original_value.change_decimals(3).unwrap();
        assert_eq!(&BigInt::from(123), result2.value());
        assert_eq!(3, result2.decimals());

        let result3 = original_value.change_decimals(5).unwrap();
        assert_eq!(original_value.value(), result3.value());

        assert!(original_value.change_decimals(2).is_err());
    }

    #[test]
    fn test_big_decimal_constructor() {
        let value = BigDecimal::new(BigInt::from(45600), 7);
        assert_eq!(&BigInt::from(45600), value.value());
        assert_eq!(7, value.decimals());

        let value = BigDecimal::new(BigInt::from(0), 5);
        assert_eq!(&BigInt::from(0), value.value());
        assert_eq!(5, value.decimals());

        let value = BigDecimal::new(BigInt::from(-10), 0);
        assert_eq!(&BigInt::from(-10), value.value());
        assert_eq!(0, value.decimals());
    }
}
