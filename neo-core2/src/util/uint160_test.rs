use crate::util::{Uint160, Uint160DecodeStringLE, Uint160DecodeStringBE, Uint160DecodeBytesLE, Uint160DecodeBytesBE};
use hex;
use serde_json;
use serde_yaml;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uint160_unmarshal_json() {
        let str = "0263c1de100292813b5e075e585acc1bae963b2d";
        let expected = Uint160DecodeStringLE(str).unwrap();

        // UnmarshalJSON decodes hex-strings
        let u1: Uint160 = serde_json::from_str(&format!("\"{}\"", str)).unwrap();
        assert_eq!(expected, u1);

        let u2: Uint160 = serde_json::from_str(&serde_json::to_string(&expected).unwrap()).unwrap();
        assert_eq!(expected, u2);

        assert!(serde_json::from_str::<Uint160>("123").is_err());
    }

    #[test]
    fn test_uint160_unmarshal_yaml() {
        let str = "0263c1de100292813b5e075e585acc1bae963b2d";
        let expected = Uint160DecodeStringLE(str).unwrap();

        let u1: Uint160 = serde_yaml::from_str(&format!("\"{}\"", str)).unwrap();
        assert_eq!(expected, u1);

        let data = serde_yaml::to_string(&u1).unwrap();
        let u2: Uint160 = serde_yaml::from_str(&data).unwrap();
        assert_eq!(expected, u2);

        assert!(serde_yaml::from_str::<Uint160>("[]").is_err());
    }

    #[test]
    fn test_uint160_decode_string() {
        let hex_str = "2d3b96ae1bcc5a585e075e3b81920210dec16302";
        let val = Uint160DecodeStringBE(hex_str).unwrap();
        assert_eq!(hex_str, val.to_string());

        let val_le = Uint160DecodeStringLE(hex_str).unwrap();
        assert_eq!(val, val_le.reverse());

        assert!(Uint160DecodeStringBE(&hex_str[1..]).is_err());
        assert!(Uint160DecodeStringLE(&hex_str[1..]).is_err());

        let invalid_hex_str = "zz3b96ae1bcc5a585e075e3b81920210dec16302";
        assert!(Uint160DecodeStringBE(invalid_hex_str).is_err());
        assert!(Uint160DecodeStringLE(invalid_hex_str).is_err());
    }

    #[test]
    fn test_uint160_decode_bytes() {
        let hex_str = "2d3b96ae1bcc5a585e075e3b81920210dec16302";
        let b = hex::decode(hex_str).unwrap();

        let val = Uint160DecodeBytesBE(&b).unwrap();
        assert_eq!(hex_str, val.to_string());

        let val_le = Uint160DecodeBytesLE(&b).unwrap();
        assert_eq!(val, val_le.reverse());

        assert!(Uint160DecodeBytesLE(&b[1..]).is_err());
        assert!(Uint160DecodeBytesBE(&b[1..]).is_err());
    }

    #[test]
    fn test_uint160_equals() {
        let a = "2d3b96ae1bcc5a585e075e3b81920210dec16302";
        let b = "4d3b96ae1bcc5a585e075e3b81920210dec16302";

        let ua = Uint160DecodeStringBE(a).unwrap();
        let ub = Uint160DecodeStringBE(b).unwrap();

        assert_ne!(ua, ub, "{} and {} cannot be equal", ua, ub);
        assert_eq!(ua, ua, "{} and {} must be equal", ua, ua);
    }

    #[test]
    fn test_uint160_less() {
        let a = "2d3b96ae1bcc5a585e075e3b81920210dec16302";
        let b = "2d3b96ae1bcc5a585e075e3b81920210dec16303";

        let ua = Uint160DecodeStringBE(a).unwrap();
        let ua2 = Uint160DecodeStringBE(a).unwrap();
        let ub = Uint160DecodeStringBE(b).unwrap();

        assert!(ua < ub);
        assert!(!(ua < ua2));
        assert!(!(ub < ua));
        assert_eq!(ua.cmp(&ub), std::cmp::Ordering::Less);
        assert_eq!(ub.cmp(&ua), std::cmp::Ordering::Greater);
        assert_eq!(ua.cmp(&ua), std::cmp::Ordering::Equal);
        assert_eq!(ub.cmp(&ub), std::cmp::Ordering::Equal);
    }

    #[test]
    fn test_uint160_string() {
        let hex_str = "b28427088a3729b2536d10122960394e8be6721f";
        let hex_rev_str = "1f72e68b4e39602912106d53b229378a082784b2";

        let val = Uint160DecodeStringBE(hex_str).unwrap();

        assert_eq!(hex_str, val.to_string());
        assert_eq!(hex_rev_str, val.to_string_le());
    }

    #[test]
    fn test_uint160_reverse() {
        let hex_str = "b28427088a3729b2536d10122960394e8be6721f";
        let val = Uint160DecodeStringBE(hex_str).unwrap();

        assert_eq!(hex_str, val.reverse().to_string_le());
        assert_eq!(val, val.reverse().reverse());
    }
}
