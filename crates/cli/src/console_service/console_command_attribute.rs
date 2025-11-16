/// Mirrors `Neo.ConsoleService.ConsoleCommandAttribute`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleCommandAttribute {
    verbs: Vec<String>,
    category: String,
    description: String,
}

impl ConsoleCommandAttribute {
    /// Creates a new attribute from a whitespace-delimited set of verbs.
    pub fn new(verbs: &str) -> Self {
        let verbs = verbs
            .split_whitespace()
            .filter(|verb| !verb.is_empty())
            .map(|verb| verb.to_ascii_lowercase())
            .collect();
        Self {
            verbs,
            category: String::new(),
            description: String::new(),
        }
    }

    pub fn verbs(&self) -> &[String] {
        &self.verbs
    }

    pub fn category(&self) -> &str {
        &self.category
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbs_are_lowercased_and_split() {
        let attr = ConsoleCommandAttribute::new("Open Wallet");
        assert_eq!(attr.verbs(), &["open".to_string(), "wallet".to_string()]);
    }

    #[test]
    fn category_and_description_builders_work() {
        let attr = ConsoleCommandAttribute::new("deploy")
            .with_category("Contracts")
            .with_description("Deploy a contract");
        assert_eq!(attr.category(), "Contracts");
        assert_eq!(attr.description(), "Deploy a contract");
    }
}
