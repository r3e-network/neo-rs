# Architecture Decision Records

This directory contains Architecture Decision Records (ADRs) for the neo-rs project.

## What is an ADR?

An Architecture Decision Record (ADR) captures an important architectural decision made along with its context and consequences.

## Format

Each ADR follows this structure:
- **Title**: Short descriptive title
- **Status**: Proposed | Accepted | Deprecated | Superseded
- **Date**: When the decision was made
- **Context**: The issue motivating this decision
- **Decision**: The change we're proposing or have made
- **Consequences**: What becomes easier or harder as a result

## Index

| ADR | Title | Status |
|-----|-------|--------|
| [ADR-001](001-decouple-neo-core.md) | Decouple neo-core into focused crates | Accepted (In Progress) |
| [ADR-002](002-io-persistence-crypto-facades.md) | Extract IO/persistence/crypto to separate crates | Accepted (Completed) |
| [ADR-003](003-consensus-independence.md) | Make neo-consensus independent from neo-core | Accepted (Completed) |
| [ADR-004](004-p2p-layer-extraction.md) | Extract P2P networking layer | Accepted (In Progress) |
| [ADR-005](005-verifiable-trait-primitives.md) | Move Verifiable trait to neo-primitives | Accepted (Completed) |
| [ADR-006](006-layered-architecture.md) | Layered architecture with unidirectional dependencies | Accepted (In Progress) |

## Reading Guide

If you're new to the neo-rs architecture, read in this order:

1. **ADR-006** - Start here to understand the overall layered architecture
2. **ADR-001** - Understand why neo-core is being decoupled
3. **ADR-002** - Foundation layers (IO/storage/crypto)
4. **ADR-005** - Verifiable trait (key architectural pattern)
5. **ADR-003** - Consensus independence (application of the pattern)
6. **ADR-004** - P2P extraction (application of the pattern)
