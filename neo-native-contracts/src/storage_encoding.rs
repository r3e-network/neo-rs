//! Native-contract storage encoding helpers.

/// Encodes a `BigInteger` for native-contract storage exactly like C#
/// `StorageItem`/`BigInteger.ToByteArrayStandard()`: **empty bytes for zero**,
/// otherwise the signed little-endian two's-complement form. `num-bigint`'s
/// `to_signed_bytes_le()` matches the non-zero form but yields `[0x00]` for
/// zero, which would diverge the raw stored bytes (and so the state root)
/// anywhere a stored counter or setting can legitimately reach zero (e.g.
/// `_votersCount` after the last un-vote, `gasPerBlock = 0`, `feePerByte = 0`).
/// Reads are unaffected: `BigInt::from_signed_bytes_le(&[])` is zero.
pub(crate) fn bigint_to_storage_bytes(value: &num_bigint::BigInt) -> Vec<u8> {
    use num_traits::Zero;
    if value.is_zero() {
        Vec::new()
    } else {
        value.to_signed_bytes_le()
    }
}
