## ADDED Requirements

### Requirement: Single consensus VM authority
The system SHALL execute Neo N3 scripts only through the workspace `neo-vm` interpreter and SHALL use `StackItem` as the only mutable runtime value model.

#### Scenario: Canonical block execution
- **WHEN** a block transaction or native-contract hook executes a script
- **THEN** execution SHALL not invoke another interpreter or convert the runtime object graph through `StackValue`

### Requirement: Hardfork-correct script validation
The system SHALL distinguish relaxed historical contract scripts from strict scripts exactly as Neo v3.10.1 does.

#### Scenario: Relaxed script has unreachable malformed trailing bytes
- **WHEN** a pre-Basilisk contract executes a valid entry point that returns before malformed trailing bytes
- **THEN** the reached instructions SHALL execute without eagerly rejecting the unreachable bytes

#### Scenario: Runtime-loaded script is structurally invalid
- **WHEN** `System.Runtime.LoadScript` receives a script with an invalid jump target, operand, or forbidden strict instruction form
- **THEN** the syscall SHALL fault before loading or executing that script

### Requirement: Return-count parity
The system SHALL enforce the declared return-value count for both explicit and implicit `RET` operations.

#### Scenario: End of script has the wrong number of values
- **WHEN** execution reaches the byte immediately after the script with an evaluation-stack size different from the frame return count
- **THEN** execution SHALL enter `FAULT` through the normal `RET` behavior

### Requirement: Control-flow target parity
Execution contexts SHALL accept instruction positions from zero through the script length inclusive and SHALL reject positions greater than the script length. Jump opcodes SHALL retain the stricter Neo.VM rule that their target is less than the script length.

#### Scenario: CALL targets script end
- **WHEN** `CALL` targets exactly `script.len()`
- **THEN** the callee SHALL execute an implicit `RET` and apply normal return-count validation

#### Scenario: JMP targets script end
- **WHEN** a jump opcode targets exactly `script.len()`
- **THEN** execution SHALL fault with an out-of-range jump

#### Scenario: Exception target exceeds script end
- **WHEN** `TRY` or `ENDTRY` resolves a target greater than `script.len()`
- **THEN** execution SHALL fault rather than treating the target as an implicit return

### Requirement: Fault artifact parity
The system SHALL apply Neo v3.10.1 ApplicationEngine fault cleanup before publishing execution artifacts.

#### Scenario: Notification precedes a fault
- **WHEN** a script emits a notification and later enters `FAULT`
- **THEN** the resulting application execution SHALL contain no notifications and SHALL retain the fault state and exception

### Requirement: VM operation API parity
Public VM operations used by execution, tooling, or script construction SHALL preserve official Neo.VM v3.10.1 type, opcode, and mutation-order behavior.

#### Scenario: Null conversion to a reference type
- **WHEN** `Null` is converted to Map, Pointer, or InteropInterface through the VM conversion API
- **THEN** the result SHALL remain `Null`

#### Scenario: Script builder pushes a struct
- **WHEN** the script builder emits a compound Struct stack item
- **THEN** it SHALL emit `PACKSTRUCT` rather than `PACK`

#### Scenario: Slot store has an invalid destination
- **WHEN** a slot-store operation uses an invalid slot or index
- **THEN** it SHALL fault without popping the source operand

#### Scenario: Throw has no handler
- **WHEN** an exception is thrown and no invocation frame contains a matching handler
- **THEN** the invocation frames SHALL remain available in the same state as official Neo.VM fault diagnostics
