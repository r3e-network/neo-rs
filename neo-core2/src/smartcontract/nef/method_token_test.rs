use std::str::repeat;
use crate::internal::random;
use crate::internal::testserdes;
use crate::smartcontract::callflag;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smartcontract::nef::{MethodToken, MAX_METHOD_LENGTH};

    #[test]
    fn test_method_token_serializable() {
        fn get_token() -> MethodToken {
            MethodToken {
                hash: random::uint160(),
                method: "MethodName".to_string(),
                param_count: 2,
                has_return: true,
                call_flag: callflag::READ_STATES,
            }
        }

        #[test]
        fn good() {
            testserdes::encode_decode_binary(&get_token(), &MethodToken::default());
        }

        #[test]
        fn too_long_name() {
            let mut tok = get_token();
            tok.method = repeat("s", MAX_METHOD_LENGTH + 1);
            let data = testserdes::encode_binary(&tok).unwrap();
            assert!(testserdes::decode_binary::<MethodToken>(&data).is_err());
        }

        #[test]
        fn start_with_underscore() {
            let mut tok = get_token();
            tok.method = "_method".to_string();
            let data = testserdes::encode_binary(&tok).unwrap();
            let err = testserdes::decode_binary::<MethodToken>(&data).unwrap_err();
            assert!(err.to_string().contains("invalid method name"));
        }

        #[test]
        fn invalid_call_flag() {
            let mut tok = get_token();
            tok.call_flag = !callflag::ALL;
            let data = testserdes::encode_binary(&tok).unwrap();
            let err = testserdes::decode_binary::<MethodToken>(&data).unwrap_err();
            assert!(err.to_string().contains("invalid call flag"));
        }
    }
}
