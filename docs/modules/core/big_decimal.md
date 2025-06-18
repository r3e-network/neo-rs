# BigDecimal Implementation

This document details the implementation of the BigDecimal type in Rust.

## Overview

BigDecimal is a fixed-point number of arbitrary precision used in the Neo blockchain for:

- Token balances
- Native contract calculations
- Financial operations

## C# Implementation

In the C# implementation, BigDecimal is a struct with:

- A BigInteger value field
- A byte decimals field
- Methods for changing precision
- Parsing from strings
- Formatting to strings
- Comparison and equality

## Rust Implementation

In the Rust implementation, BigDecimal is a struct with:

- A BigInt value field
- A byte decimals field
- Methods for changing precision
- Parsing from strings
- Formatting to strings
- Comparison and equality via standard traits

### BigDecimal Structure

```rust
pub struct BigDecimal {
    value: BigInt,
    decimals: u8,
}
```

## Key Features

### Creation and Conversion

BigDecimal provides methods for:

- Creating from BigInt and decimals
- Changing the number of decimal places
- Parsing from strings with scientific notation support

### Precision Handling

BigDecimal handles precision with:

- Fixed number of decimal places
- Ability to change precision (increase or decrease)
- Validation to prevent precision loss

### String Conversion

BigDecimal provides:

- FromStr implementation for parsing from strings
- Display implementation for formatting to strings
- Support for scientific notation in parsing

### Comparison and Equality

BigDecimal implements standard Rust traits:

- PartialEq for equality comparison
- Eq for reflexive equality
- PartialOrd for ordering comparison
- Ord for total ordering

## Usage Examples

### Creating BigDecimal

```rust
// From BigInt and decimals
let value = BigInt::from(12345);
let decimals = 2;
let bd = BigDecimal::new(value, decimals);

// From string
let bd = BigDecimal::parse("123.45", 2).unwrap();
let bd_sci = BigDecimal::parse("1.2345e2", 2).unwrap();
```

### Changing Precision

```rust
let bd = BigDecimal::new(BigInt::from(12345), 2);

// Increase precision
let increased = bd.change_decimals(4).unwrap();
assert_eq!(increased.value(), &BigInt::from(1234500));
assert_eq!(increased.decimals(), 4);

// Decrease precision (if possible without loss)
let decreased = increased.change_decimals(2).unwrap();
assert_eq!(decreased.value(), &BigInt::from(12345));
assert_eq!(decreased.decimals(), 2);
```

### Formatting to String

```rust
let bd = BigDecimal::new(BigInt::from(12345), 2);
println!("{}", bd); // "123.45"

let bd = BigDecimal::new(BigInt::from(12300), 2);
println!("{}", bd); // "123" (trailing zeros removed)

let bd = BigDecimal::new(BigInt::from(-12345), 2);
println!("{}", bd); // "-123.45"
```

### Comparison

```rust
let bd1 = BigDecimal::new(BigInt::from(12345), 2);
let bd2 = BigDecimal::new(BigInt::from(12345), 2);
let bd3 = BigDecimal::new(BigInt::from(12346), 2);
let bd4 = BigDecimal::new(BigInt::from(123450), 3);

assert_eq!(bd1, bd2);
assert!(bd1 < bd3);
assert_eq!(bd1, bd4); // Same value with different precision
```

## Implementation Differences

### Decimal Representation

- C#: Uses BigInteger for the value
- Rust: Uses BigInt for the value

### String Parsing

- C#: Custom parsing logic
- Rust: Similar custom parsing with Result return

### Precision Handling

- C#: Similar approach with decimal places
- Rust: Same approach with validation

## Performance Considerations

- Efficient comparison by normalizing to the same number of decimals
- Optimized string formatting with trailing zero removal
- Validation to prevent precision loss

## Testing

BigDecimal has comprehensive tests for:

- Creation and conversion
- Changing precision
- Parsing from strings
- Formatting to strings
- Comparison and equality
- Error handling
