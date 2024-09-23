#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn test_from_id_valid() {
        let id = to_id(names[0].as_bytes());
        let name = from_id(id).unwrap();
        assert_eq!(names[0], name);
    }

    #[test]
    fn test_from_id_invalid() {
        let err = from_id(0x42424242).unwrap_err();
        assert!(matches!(err, err_not_found));
    }
}
