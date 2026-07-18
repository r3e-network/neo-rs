# Upstream v3.10.1 Sources

These immutable revisions are the behavioral authority for this change:

- Neo: `d10e9ceecdabe3fcff719ee68ea5b76ba7e62c3d`
- Neo.VM: `004cd6070a940405818d9357638277dd44407e2e`

| Corrected behavior | Pinned upstream source |
|---|---|
| Pre-Gorgon and pre-Echidna jump-table composition, including pre-543 collection handlers, vulnerable pre-567 shifts, and vulnerable `SUBSTR` | Neo `src/Neo/SmartContract/ApplicationEngine.cs:290-313,716-741` |
| Synthetic end-of-script `RET` and normal instruction dispatch | Neo.VM `src/Neo.VM/ExecutionEngine.cs:147-169` |
| Exact `RVCount` enforcement and return propagation | Neo.VM `src/Neo.VM/JumpTable/JumpTable.Control.cs:528-545` |
| Relaxed lazy parsing versus strict whole-script validation | Neo.VM `src/Neo.VM/Script.cs:59-136` |
| Pre/post-Basilisk deployment validation selection | Neo `src/Neo/SmartContract/Native/ContractManagement.cs:274-277` |
| Always-strict `System.Runtime.LoadScript` | Neo `src/Neo/SmartContract/ApplicationEngine.Runtime.cs:217-231` |
| Inclusive context instruction-pointer bound | Neo.VM `src/Neo.VM/ExecutionContext.cs:70-81` |
| `CALL`, `ENDTRY`, strict jump, and `TRY` target assignment | Neo.VM `src/Neo.VM/JumpTable/JumpTable.Control.cs:568-645` |
| Unhandled throw frame preservation | Neo.VM `src/Neo.VM/JumpTable/JumpTable.Control.cs:653-692` |
| Notification cleanup on application fault | Neo `src/Neo/SmartContract/ApplicationEngine.cs:567-571` |
| `Null.ConvertTo` for all defined stack-item types | Neo.VM `src/Neo.VM/Types/Null.cs:27-31` |
| Struct construction uses `PACKSTRUCT` | Neo `src/Neo/Extensions/VM/ScriptBuilderExtensions.cs:99-107` |
| Slot destination validation precedes `Pop` | Neo.VM `src/Neo.VM/JumpTable/JumpTable.Slot.cs:654-660` |
| Strict UTF-8 stack-item strings | Neo.VM `src/Neo.VM/Types/StackItem.cs:197-203` |
| Strict UTF-8 decoder fallback | Neo.VM `src/Neo.VM/Utility.cs:21-31,54-60` |
| `ABORTMSG` state and exception text | Neo.VM `tests/Neo.VM.Tests/Tests/OpCodes/Control/ABORTMSG.json` |
| Per-transaction engine construction and Policy fee refresh from the latest same-block snapshot | Neo `src/Neo/Ledger/Blockchain.cs:410-453`; Neo `src/Neo/SmartContract/ApplicationEngine.cs:232-268` |

The local regression comments cite the relevant entry above. Reth and
Polkadot/Substrate remain architecture and performance references only; they
are not Neo protocol authorities.

[`csharp-differential-evidence.md`](csharp-differential-evidence.md) records
the immutable generated fixture corpus, hardfork coverage, and Rust consumer
results for these sources.
