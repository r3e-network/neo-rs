## ADDED Requirements

### Requirement: Critical path benchmarks

The system SHALL provide benchmarks for performance-critical operations.

#### Scenario: Benchmark block processing

- **WHEN** block processing benchmark is executed
- **THEN** system reports average time to process a block

### Requirement: Regression detection

The system SHALL detect performance regressions in CI.

#### Scenario: Detect performance regression

- **WHEN** benchmarks run in CI
- **THEN** system fails build if performance degrades by >10%

### Requirement: Baseline comparison

The system SHALL compare current performance against baseline.

#### Scenario: Compare with baseline

- **WHEN** benchmarks complete
- **THEN** system shows percentage change from baseline for each metric
