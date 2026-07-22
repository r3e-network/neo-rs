# Retained protocol and signer evidence

This directory contains compact historical Neo C# parity records and the HSM
designs referenced by `neo-gui`. It is not an implementation backlog.

Current architecture, requirements, and work status live in `AGENTS.md`,
`docs/`, and `openspec/`. Workspace `neo-vm` is the sole VM semantic authority;
references in the historical records to removed crates or paths describe the
code reviewed at that time and are not current guidance.

## Protocol evidence

- `consensus-findings-reverify-2026-05-30.md`: adversarial re-verification of
  early consensus-parity findings.
- `interop-findings-reverify-2026-05-30.md`: adversarial re-verification of
  early wire/interoperability findings.
- `neo-v3100-parity-plan.md`: completed v3.10.0 hardfork-parity migration record.

## Signer security design

- `aws-hsm-nitro-tee-design.md`: AWS HSM and Nitro design background.
- `multicloud-hsm-design.md`: multi-cloud signing design background.
