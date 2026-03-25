## ADDED Requirements

### Requirement: Metrics collection

The system SHALL collect operational metrics for monitoring.

#### Scenario: Collect node metrics

- **WHEN** node is running
- **THEN** system exposes metrics for block height, sync status, peer count, and memory usage

### Requirement: Structured logging

The system SHALL provide structured logging for operational visibility.

#### Scenario: Log critical events

- **WHEN** critical events occur
- **THEN** system logs with structured fields (timestamp, level, component, message)

### Requirement: Health check endpoint

The system SHALL provide health check endpoint for monitoring systems.

#### Scenario: Check node health

- **WHEN** health endpoint is queried
- **THEN** system returns status indicating if node is healthy and synced
