//! JSONPath filter execution for oracle responses.

use crate::error::OracleServiceError;
use neo_json::JToken;

/// Maximum nesting depth for JSON parsing in oracle filters.
pub const FILTER_MAX_NEST: usize = 64;

/// Applies a JSONPath filter to the input string.
///
/// If `filter` is `None` or empty, returns the raw input bytes.
/// Otherwise parses the input as JSON, applies the JSONPath expression,
/// and serializes the result back to bytes.
pub fn filter_json(input: &str, filter: Option<&str>) -> Result<Vec<u8>, OracleServiceError> {
    if filter.map(|value| value.is_empty()).unwrap_or(true) {
        return Ok(input.as_bytes().to_vec());
    }

    let token = JToken::parse(input, FILTER_MAX_NEST)
        .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
    let array = token
        .json_path(filter.unwrap_or(""))
        .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
    let token = JToken::from(array);
    token
        .to_byte_array(false)
        .map_err(|err| OracleServiceError::Processing(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_matches_csharp_examples() {
        let json = r#"
        {
            "Stores": ["Lambton Quay",  "Willis Street"],
            "Manufacturers": [{
                "Name": "Acme Co",
                "Products": [{ "Name": "Anvil", "Price": 50 }]
            },{
                "Name": "Contoso",
                "Products": [
                    { "Name": "Elbow Grease", "Price": 99.95 },
                    { "Name": "Headlight Fluid", "Price": 4 }
                ]
            }]
        }
        "#;

        assert_eq!(
            r#"["Acme Co"]"#,
            String::from_utf8(filter_json(json, Some("$.Manufacturers[0].Name")).unwrap()).unwrap()
        );
        assert_eq!(
            "[50]",
            String::from_utf8(
                filter_json(json, Some("$.Manufacturers[0].Products[0].Price")).unwrap()
            )
            .unwrap()
        );
        assert_eq!(
            r#"["Elbow Grease"]"#,
            String::from_utf8(
                filter_json(json, Some("$.Manufacturers[1].Products[0].Name")).unwrap()
            )
            .unwrap()
        );
        assert_eq!(
            r#"[{"Name":"Elbow Grease","Price":99.95}]"#,
            String::from_utf8(filter_json(json, Some("$.Manufacturers[1].Products[0]")).unwrap())
                .unwrap()
        );
    }

    #[test]
    fn filter_returns_raw_bytes_when_no_filter() {
        let input = r#"{"key": "value"}"#;
        assert_eq!(filter_json(input, None).unwrap(), input.as_bytes().to_vec());
        assert_eq!(
            filter_json(input, Some("")).unwrap(),
            input.as_bytes().to_vec()
        );
    }
}
