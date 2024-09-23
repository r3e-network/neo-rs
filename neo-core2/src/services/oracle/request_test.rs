#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;

    #[test]
    fn test_check_content_type() {
        let allowed_types = vec!["application/json", "text/plain"];
        assert!(check_media_type("application/json", &allowed_types));
        assert!(check_media_type("application/json; param=value", &allowed_types));
        assert!(check_media_type("text/plain; filename=file.txt", &allowed_types));

        assert!(!check_media_type("image/gif", &allowed_types));
        assert!(check_media_type("image/gif", &None));

        assert!(!check_media_type("invalid format", &allowed_types));
    }
}
