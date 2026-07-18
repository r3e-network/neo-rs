## MODIFIED Requirements

### Requirement: Protocol version compliance
The system SHALL implement Neo N3 v3.10.1 protocol behavior with byte-for-byte wire compatibility and state-transition compatibility to the official C# reference implementation. A compatibility claim SHALL identify the differential corpus, hardfork coverage, replay range, and state-root evidence used to support it.

#### Scenario: Block processing matches C# implementation
- **WHEN** processing a block from MainNet
- **THEN** the resulting acceptance decision, execution results, persisted state, and state root SHALL match C# node output exactly

#### Scenario: Transaction validation matches C# implementation
- **WHEN** validating a transaction
- **THEN** the validation result SHALL match the C# node for all covered edge cases

#### Scenario: VM differential fixture executes
- **WHEN** a fixture derived from official Neo or Neo.VM v3.10.1 executes
- **THEN** VM state, result stack, invocation behavior, exception outcome, notifications, and relevant state changes SHALL match the recorded C# result

#### Scenario: Compatibility status is reported
- **WHEN** documentation or automation reports v3.10.1 compatibility as complete
- **THEN** the full required differential suite and declared MainNet replay/state-root gates SHALL have completed successfully rather than being skipped or inferred
