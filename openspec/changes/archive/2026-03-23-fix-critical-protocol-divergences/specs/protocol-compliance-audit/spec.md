## ADDED Requirements

### Requirement: Fix verification
The system SHALL verify each protocol fix against C# test vectors.

#### Scenario: Block validation fix verified
- **WHEN** block validation fix is applied
- **THEN** all C# test vectors SHALL pass

### Requirement: Regression prevention
The system SHALL include regression tests for all fixed issues.

#### Scenario: Regression test added
- **WHEN** a fix is implemented
- **THEN** regression test SHALL be added before merge
