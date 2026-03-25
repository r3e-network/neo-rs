## ADDED Requirements

### Requirement: Environment-based configuration
The system SHALL support configuration via TOML files with environment variable overrides.

#### Scenario: Configuration loading
- **WHEN** node starts
- **THEN** configuration SHALL be loaded from file and validated

#### Scenario: Environment override
- **WHEN** environment variable is set
- **THEN** it SHALL override corresponding config file value

### Requirement: Configuration validation
The system SHALL validate configuration at startup and fail fast on invalid config.

#### Scenario: Invalid configuration
- **WHEN** configuration contains invalid values
- **THEN** node SHALL exit with clear error message

### Requirement: Secrets management
The system SHALL support secure secrets management without hardcoded credentials.

#### Scenario: Secret loading
- **WHEN** loading secrets
- **THEN** secrets SHALL be loaded from secure storage or environment
