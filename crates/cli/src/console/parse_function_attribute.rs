/// Marker struct mirroring `ParseFunctionAttribute` in the C# CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseFunctionAttribute {
    pub description: String,
}

impl ParseFunctionAttribute {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_description() {
        let attr = ParseFunctionAttribute::new("unit test");
        assert_eq!(attr.description, "unit test");
    }
}
