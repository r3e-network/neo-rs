use serde_json::json;
use serde_json::from_slice;
use serde::Deserialize;

#[derive(Deserialize)]
struct Validator {
    publickey: String,
    votes: i64,
    active: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_json_diff::assert_json_eq;

    #[test]
    fn test_validator_unmarshal() {
        let old = br#"{"publickey":"02a7bc55fe8684e0119768d104ba30795bdcc86619e864add26156723ed185cd62","votes":"100500","active":true}"#;
        let mut v: Validator = from_slice(old).unwrap();
        assert_eq!(v.votes, 100500);

        let new_v = br#"{"publickey":"02a7bc55fe8684e0119768d104ba30795bdcc86619e864add26156723ed185cd62","votes":42}"#;
        v = from_slice(new_v).unwrap();
        assert_eq!(v.votes, 42);

        let bad = br#"{"publickey":"02a7bc55fe8684e0119768d104ba30795bdcc86619e864add26156723ed185cd62","votes":"notanumber"}"#;
        assert!(from_slice::<Validator>(bad).is_err());
    }
}
