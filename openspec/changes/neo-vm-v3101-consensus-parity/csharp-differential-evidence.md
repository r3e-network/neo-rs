# C# Differential Evidence

## Authorities

- Neo.VM v3.10.1: `004cd6070a940405818d9357638277dd44407e2e`
- Neo v3.10.1: `d10e9ceecdabe3fcff719ee68ea5b76ba7e62c3d`
- Generator runtime: .NET SDK `10.0.100`

The generators under `scripts/oracles/v3101/` reference local checkouts of
those immutable revisions. They are evidence tooling only and are not part of
the Rust production dependency graph.

## Recorded Corpus

| Fixture | Cases | Coverage |
|---|---:|---|
| `neo-vm/tests/fixtures/csharp-v3.10.1-vm.json` | 23 | implicit `RET` and exact `RVCount`; lazy/strict parsing; context, call, jump, `TRY`, and `ENDTRY` bounds; `Null` conversions; uninitialized and out-of-range slot stores; unhandled throws; strict `ABORTMSG` UTF-8 |
| `neo-execution/tests/fixtures/csharp-v3.10.1-application.json` | 9 | always-strict runtime loading before and after Basilisk; fault notification cleanup; struct emission; pre-Echidna, Echidna-to-Gorgon, and Gorgon-and-later jump tables |

Hardfork applicability explicitly covers pre-Basilisk historical contract
loading, Basilisk-and-later strict contract loading, runtime loading in both
eras, the pre-Echidna vulnerable `SUBSTR` table, the Echidna-to-Gorgon table,
and the default Gorgon-and-later table.

## Verification

Fresh C# generator output was compared to every checked-in `observed` object:

```text
verified 23 recorded cases from /dev/stdin
verified 9 recorded cases from /dev/stdin
```

The Rust consumers passed:

```text
cargo test -p neo-vm csharp_v3101 --lib
5 passed; 0 failed

cargo test -p neo-execution csharp_v3101 --lib
13 passed; 0 failed
```

The filtered totals include existing v3.10.1 source-derived regressions in
addition to the four new data-driven test groups in each crate. Fixture ID-set
tests fail closed on missing, duplicate, or unreviewed semantic cases.
