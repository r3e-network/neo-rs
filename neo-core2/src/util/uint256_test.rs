use crate::util::{Uint256, Uint256DecodeStringLE, Uint256DecodeStringBE, Uint256DecodeBytesLE, Uint256DecodeBytesBE};
use hex;
use serde_json;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uint256_unmarshal_json() {
        let str = "f037308fa0ab18155bccfc08485468c112409ea5064595699e98c545f245f32d";
        let expected = Uint256DecodeStringLE(str).unwrap();

        // UnmarshalJSON decodes hex-strings
        let u1: Uint256 = serde_json::from_str(&format!("\"{}\"", str)).unwrap();
        assert_eq!(expected, u1);

        let u2: Uint256 = serde_json::from_str(&serde_json::to_string(&expected).unwrap()).unwrap();
        assert_eq!(expected, u2);

        // UnmarshalJSON does not accept numbers
        assert!(serde_json::from_str::<Uint256>("123").is_err());
    }

    #[test]
    fn test_uint256_decode_string() {
        let hex_str = "f037308fa0ab18155bccfc08485468c112409ea5064595699e98c545f245f32d";
        let val = Uint256DecodeStringLE(hex_str).unwrap();
        assert_eq!(hex_str, val.to_string_le());

        let val_be = Uint256DecodeStringBE(hex_str).unwrap();
        assert_eq!(val, val_be.reverse());

        let bs = hex::decode(hex_str).unwrap();

        let val1 = Uint256DecodeBytesBE(&bs).unwrap();
        assert_eq!(hex_str, val1.to_string());
        assert_eq!(val, val1.reverse());

        assert!(Uint256DecodeStringLE(&hex_str[1..]).is_err());
        assert!(Uint256DecodeStringBE(&hex_str[1..]).is_err());

        let invalid_hex_str = "zzz7308fa0ab18155bccfc08485468c112409ea5064595699e98c545f245f32d";
        assert!(Uint256DecodeStringLE(invalid_hex_str).is_err());
        assert!(Uint256DecodeStringBE(invalid_hex_str).is_err());
    }

    #[test]
    fn test_uint256_decode_bytes() {
        let hex_str = "f037308fa0ab18155bccfc08485468c112409ea5064595699e98c545f245f32d";
        let b = hex::decode(hex_str).unwrap();

        let val = Uint256DecodeBytesLE(&b).unwrap();
        assert_eq!(hex_str, val.to_string_le());

        assert!(Uint256DecodeBytesBE(&b[1..]).is_err());
    }

    #[test]
    fn test_uint256_equals() {
        let a = "f037308fa0ab18155bccfc08485468c112409ea5064595699e98c545f245f32d";
        let b = "e287c5b29a1b66092be6803c59c765308ac20287e1b4977fd399da5fc8f66ab5";

        let ua = Uint256DecodeStringLE(a).unwrap();
        let ub = Uint256DecodeStringLE(b).unwrap();

        assert!(!ua.equals(&ub), "{} and {} cannot be equal", ua, ub);
        assert!(ua.equals(&ua), "{} and {} must be equal", ua, ua);
        assert_eq!(ua.compare(&ua), std::cmp::Ordering::Equal, "{} and {} must be equal", ua, ua);
    }

    #[test]
    fn test_uint256_serializable() {
        let a = Uint256::from([
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
            17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32,
        ]);

        let encoded = bincode::serialize(&a).unwrap();
        let b: Uint256 = bincode::deserialize(&encoded).unwrap();
        assert_eq!(a, b);
    }

    #[bench]
    fn bench_uint256_decode_string_le(b: &mut test::Bencher) {
        let a = "f037308fa0ab18155bccfc08485468c112409ea5064595699e98c545f245f32d";

        b.iter(|| {
            Uint256DecodeStringLE(a).unwrap();
        });
    }
}
