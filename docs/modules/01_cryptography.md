# Cryptography Module Conversion

This document details the conversion of the Neo N3 C# Cryptography module to Rust.

## Module Overview

The Cryptography module provides essential cryptographic primitives for the Neo blockchain, including:

- Elliptic curve cryptography (ECC)
- Hashing algorithms
- Base58 encoding/decoding
- Bloom filters
- Merkle trees

## Type Mappings

| C# Type | Rust Type | Notes |
|---------|-----------|-------|
| `byte[]` | `Vec<u8>` or `&[u8]` | Use `Vec<u8>` for owned data, `&[u8]` for borrowed data |
| `string` | `String` or `&str` | Use `String` for owned data, `&str` for borrowed data |
| `BigInteger` | `num_bigint::BigInt` | For arbitrary-precision integers |
| `static class` | Module with functions | C# static classes become Rust modules with functions |
| `class` | `struct` or `enum` | Use Rust structs or enums depending on the use case |
| `interface` | `trait` | C# interfaces become Rust traits |

## File Mappings

| C# File | Rust File | Implementation Status |
|---------|-----------|------------------------|
| `Base58.cs` | `base58.rs` | 🔴 Not Started |
| `BloomFilter.cs` | `bloom_filter.rs` | 🔴 Not Started |
| `Crypto.cs` | `crypto.rs` | 🔴 Not Started |
| `ECC/ECCurve.cs` | `ecc/curve.rs` | 🔴 Not Started |
| `ECC/ECFieldElement.cs` | `ecc/field_element.rs` | 🔴 Not Started |
| `ECC/ECPoint.cs` | `ecc/point.rs` | 🔴 Not Started |
| `Ed25519.cs` | `ed25519.rs` | 🔴 Not Started |
| `HashAlgorithm.cs` | `hash_algorithm.rs` | 🔴 Not Started |
| `Hasher.cs` | `hasher.rs` | 🔴 Not Started |
| `Helper.cs` | `helper.rs` | 🔴 Not Started |
| `MerkleTree.cs` | `merkle_tree.rs` | 🔴 Not Started |
| `MerkleTreeNode.cs` | `merkle_tree_node.rs` | 🔴 Not Started |
| `Murmur128.cs` | `murmur128.rs` | 🔴 Not Started |
| `Murmur32.cs` | `murmur32.rs` | 🔴 Not Started |
| `RIPEMD160Managed.cs` | `ripemd160.rs` | 🔴 Not Started |

## Detailed Conversion Notes

### Base58

**C# Implementation:**
- Static class with encoding/decoding methods
- Handles Bitcoin-style Base58 encoding with checksum

**Rust Implementation:**
- Module with public functions
- Use `bs58` crate for basic functionality
- Implement Neo-specific checksum handling

### BloomFilter

**C# Implementation:**
- Class for probabilistic set membership testing
- Used for efficient transaction filtering

**Rust Implementation:**
- Struct with methods
- Consider using `bloomfilter` crate or custom implementation

### ECC

**C# Implementation:**
- `ECCurve`: Represents elliptic curve parameters
- `ECFieldElement`: Element in the finite field
- `ECPoint`: Point on the elliptic curve

**Rust Implementation:**
- Consider using `secp256k1` crate for secp256k1 curve
- Implement custom types for Neo-specific functionality
- Ensure compatibility with Neo serialization format

### Hashing

**C# Implementation:**
- Various hash algorithm implementations
- `Hasher` class for common hash operations

**Rust Implementation:**
- Use `sha2`, `ripemd160` crates for standard algorithms
- Implement Neo-specific hash functions
- Ensure consistent byte ordering and encoding

### MerkleTree

**C# Implementation:**
- Tree structure for efficient verification of data integrity
- Used for transaction validation

**Rust Implementation:**
- Custom implementation with Neo-specific requirements
- Optimize for memory usage and performance

## Dependencies

- `num-bigint`: For arbitrary-precision arithmetic
- `secp256k1`: For elliptic curve cryptography
- `sha2`, `ripemd160`: For cryptographic hashing
- `bs58`: For Base58 encoding/decoding
- `rand`: For secure random number generation

## Testing Strategy

1. Convert all C# unit tests to Rust
2. Add additional tests for Rust-specific edge cases
3. Benchmark performance against C# implementation
4. Verify compatibility with Neo N3 network
