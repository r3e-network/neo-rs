use super::command_token::CommandToken;
use anyhow::{bail, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum ArgumentValue {
    String(String),
    Bool(bool),
    Int(i64),
}

impl Default for ArgumentValue {
    fn default() -> Self {
        ArgumentValue::String(String::new())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterKind {
    String,
    Bool,
    Int,
}

#[derive(Debug, Clone)]
pub struct ParameterDescriptor {
    pub name: String,
    pub kind: ParameterKind,
    pub default: Option<ArgumentValue>,
}

impl ParameterDescriptor {
    pub fn new(name: impl Into<String>, kind: ParameterKind) -> Self {
        Self {
            name: name.into(),
            kind,
            default: None,
        }
    }

    pub fn with_default(mut self, value: ArgumentValue) -> Self {
        self.default = Some(value);
        self
    }

    fn parse_value(&self, raw: &str) -> Result<ArgumentValue> {
        match self.kind {
            ParameterKind::String => Ok(ArgumentValue::String(raw.to_string())),
            ParameterKind::Bool => parse_bool(raw),
            ParameterKind::Int => parse_int(raw),
        }
    }

    fn default_value(&self) -> Option<ArgumentValue> {
        self.default.clone()
    }
}

#[derive(Debug, Clone)]
pub struct ArgumentParser {
    parameters: Vec<ParameterDescriptor>,
}

impl ArgumentParser {
    pub fn new(parameters: Vec<ParameterDescriptor>) -> Self {
        Self { parameters }
    }

    pub fn parse_sequential(&self, tokens: &[CommandToken]) -> Result<Vec<ArgumentValue>> {
        let mut values = Vec::new();
        let mut index = 0usize;
        for param in &self.parameters {
            while index < tokens.len() && tokens[index].is_white_space() {
                index += 1;
            }
            if index >= tokens.len() {
                if let Some(default) = param.default_value() {
                    values.push(default);
                    continue;
                }
                bail!("Missing value for parameter: {}", param.name);
            }
            if tokens[index].is_indicator() {
                bail!(
                    "Unexpected indicator '{}' while parsing sequential arguments",
                    tokens[index].value()
                );
            }
            let parsed = param.parse_value(tokens[index].value())?;
            values.push(parsed);
            index += 1;
        }
        Ok(values)
    }

    pub fn parse_indicator(&self, tokens: &[CommandToken]) -> Result<Vec<ArgumentValue>> {
        let mut values = self
            .parameters
            .iter()
            .map(|descriptor| descriptor.default_value())
            .collect::<Vec<_>>();

        let mut missing: Vec<String> = self
            .parameters
            .iter()
            .filter(|param| param.default.is_none())
            .map(|param| param.name.clone())
            .collect();

        let mut index = 0usize;
        while index < tokens.len() {
            let token = &tokens[index];
            if !token.is_indicator() {
                index += 1;
                continue;
            }

            let param_name = token.value().trim_start_matches("--").to_ascii_lowercase();
            let Some(param_index) = self
                .parameters
                .iter()
                .position(|param| param.name.eq_ignore_ascii_case(&param_name))
            else {
                bail!("Unknown parameter: {}", param_name);
            };

            let descriptor = &self.parameters[param_index];

            // Advance past possible whitespace token.
            index += 1;
            while index < tokens.len() && tokens[index].is_white_space() {
                index += 1;
            }

            let value = if index < tokens.len() && !tokens[index].is_indicator() {
                let parsed = descriptor.parse_value(tokens[index].value())?;
                index += 1;
                parsed
            } else if descriptor.kind == ParameterKind::Bool {
                ArgumentValue::Bool(true)
            } else {
                bail!("Missing value for parameter: {}", descriptor.name);
            };

            values[param_index] = Some(value);
            missing.retain(|name| !name.eq_ignore_ascii_case(&descriptor.name));
        }

        if !missing.is_empty() {
            bail!("Missing value for parameters: {}", missing.join(","));
        }

        Ok(values.into_iter().map(|value| value.unwrap()).collect())
    }
}

fn parse_bool(value: &str) -> Result<ArgumentValue> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "y" => Ok(ArgumentValue::Bool(true)),
        "false" | "0" | "no" | "n" => Ok(ArgumentValue::Bool(false)),
        other => bail!("Cannot parse bool value from '{}'", other),
    }
}

fn parse_int(value: &str) -> Result<ArgumentValue> {
    let parsed: i64 = value
        .parse()
        .map_err(|_| anyhow::anyhow!("Cannot parse integer value from '{}'", value))?;
    Ok(ArgumentValue::Int(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console_service::command_tokenizer::tokenize;

    fn descriptor(name: &str, kind: ParameterKind) -> ParameterDescriptor {
        ParameterDescriptor::new(name, kind)
    }

    #[test]
    fn sequential_parser_consumes_tokens_in_order() {
        let parser = ArgumentParser::new(vec![
            descriptor("name", ParameterKind::String),
            descriptor("flag", ParameterKind::Bool).with_default(ArgumentValue::Bool(false)),
        ]);
        let tokens = tokenize("neo true").unwrap();
        let values = parser.parse_sequential(&tokens).unwrap();
        assert_eq!(
            values,
            vec![
                ArgumentValue::String("neo".into()),
                ArgumentValue::Bool(true)
            ]
        );
    }

    #[test]
    fn indicator_parser_supports_flags() {
        let parser = ArgumentParser::new(vec![
            descriptor("path", ParameterKind::String),
            descriptor("password", ParameterKind::String),
            descriptor("verbose", ParameterKind::Bool).with_default(ArgumentValue::Bool(false)),
        ]);
        let tokens = tokenize("--path wallet.json --password secret --verbose").unwrap();
        let values = parser.parse_indicator(&tokens).unwrap();
        assert_eq!(
            values,
            vec![
                ArgumentValue::String("wallet.json".into()),
                ArgumentValue::String("secret".into()),
                ArgumentValue::Bool(true)
            ]
        );
    }

    #[test]
    fn indicator_missing_required_parameters_errors() {
        let parser = ArgumentParser::new(vec![descriptor("path", ParameterKind::String)]);
        let tokens = tokenize("").unwrap();
        let err = parser.parse_indicator(&tokens).unwrap_err();
        assert!(err.to_string().contains("Missing value for parameters"));
    }
}
