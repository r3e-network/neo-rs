## ADDED Requirements

### Requirement: Protocol version compliance
The system SHALL implement all Neo N3 v3.9.1 protocol features with byte-for-byte compatibility to the C# reference implementation.

#### Scenario: Block processing matches C# implementation
- **WHEN** processing a block from mainnet
- **THEN** resulting state root SHALL match C# node output exactly

#### Scenario: Transaction validation matches C# implementation
- **WHEN** validating a transaction
- **THEN** validation result SHALL match C# node for all edge cases

### Requirement: Consensus mechanism compliance
The system SHALL implement dBFT 2.0 consensus exactly as specified in Neo N3 v3.9.1.

#### Scenario: Consensus message handling
- **WHEN** receiving consensus messages
- **THEN** state transitions SHALL match C# implementation

#### Scenario: View change handling
- **WHEN** view change is triggered
- **THEN** behavior SHALL match C# implementation exactly
