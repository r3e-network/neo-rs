## ADDED Requirements

### Requirement: Protocol correctness verification snapshot
The project SHALL publish a verification report that captures, for
each Neo N3 protocol surface, the current byte-for-byte compatibility
status against the C# reference implementation. The snapshot SHALL be
re-runnable from the `openspec/changes/YYYY-MM-DD-protocol-correctness-verification/`
artefact's `tasks.md`.

#### Scenario: Verification report enumerates the 7 protocol surfaces
- **WHEN** the verification report is regenerated
- **THEN** it SHALL classify each of (wire protocol, block
  validation, transaction execution, state transitions, network / P2P,
  consensus, cryptographic compatibility) as one of
  `VERIFIED`, `PARTIAL`, or `NOT VERIFIED`
- **AND** it SHALL list the test(s) that justify the classification
- **AND** it SHALL list the gaps that need follow-up work

#### Scenario: Verification report counts the failing tests
- **WHEN** the verification report is regenerated
- **THEN** it SHALL list every failing `cargo test` by
  fully-qualified test name and crate
- **AND** it SHALL identify the root cause for clusters of
  failures (e.g. the `Verifiable::hash` impl in `neo-payloads`)

## MODIFIED Requirements

### Requirement: Protocol version compliance
The system SHALL implement all Neo N3 v3.9.2 protocol features with
byte-for-byte compatibility to the C# reference implementation.

#### Scenario: Block processing matches C# implementation
- **WHEN** processing a block from mainnet
- **THEN** resulting state root SHALL match C# node output exactly

(As of 2026-06-08, this scenario is PARTIAL: the MPT genesis
state root matches C# (`test_genesis_state_root_matches_reference`
PASS), but no real mainnet block is replayed end-to-end. See
`openspec/changes/2026-06-08-protocol-correctness-verification/verification-report.md`
section 2.4 for the detailed status.)

#### Scenario: Transaction validation matches C# implementation
- **WHEN** validating a transaction
- **THEN** validation result SHALL match C# node for all edge cases

(As of 2026-06-08, this scenario is PARTIAL: `neo-payloads`'s
`Verifiable::hash` impl interprets the unsigned preimage as the
hash and so fails the `verifiable_hash_rejects_oversized_script`
test. The fix is to call `Crypto::hash256(&self.hash_data())` in
`Verifiable::hash` for `Block`, `ExtensiblePayload`, and
`Transaction`. See verification report section 2.2.)

### Requirement: Consensus mechanism compliance
The system SHALL implement dBFT 2.0 consensus exactly as specified
in Neo N3 v3.9.2.

#### Scenario: Consensus message handling
- **WHEN** receiving consensus messages
- **THEN** state transitions SHALL match C# implementation

(As of 2026-06-08, this scenario is PARTIAL: 101 / 101
non-ignored `neo-consensus` lib tests pass (1 ignored,
`test_message_deduplication`, requires a payload-signing helper
that does not yet exist in the test crate). All message-type
round-trips, view-change thresholds, Byzantine-tolerance
scenarios, recovery-message signature checks, and
`PersistCompleted`-driven block-commit events are covered.
No multi-validator integration test exercises the full
prepare / prepare-response / commit / commit-sig-recovery cycle
end-to-end. See verification report section 2.6.)

#### Scenario: View change handling
- **WHEN** view change is triggered
- **THEN** behavior SHALL match C# implementation exactly

(As of 2026-06-08, this scenario is VERIFIED by the
`change_view_*` tests in `neo-consensus/src/service/tests/change_view.rs`:
`change_view_threshold_triggers_view_change`,
`view_change_allows_consensus_to_complete`,
`recovery_request_when_more_than_f_committed`, and
`recovery_message_change_view_triggers_view_change` all PASS.)
