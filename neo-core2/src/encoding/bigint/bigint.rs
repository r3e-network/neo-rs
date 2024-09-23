use std::cmp;
use std::panic;
use num_bigint::{BigInt, Sign};
use num_traits::Zero;

const MAX_BYTES_LEN: usize = 32; // 256-bit signed integer
const WORD_SIZE_BYTES: usize = std::mem::size_of::<usize>();

lazy_static! {
    static ref BIG_ONE: BigInt = BigInt::from(1);
}

// FromBytes converts data in little-endian format to
// an integer.
pub fn from_bytes(data: &[u8]) -> BigInt {
    let mut n = BigInt::zero();
    let size = data.len();
    if size == 0 {
        if data.is_empty() {
            panic!("nil slice provided to `FromBytes`");
        }
        return BigInt::zero();
    }

    let is_neg = data[size - 1] & 0x80 != 0;

    let size = get_effective_size(data, is_neg);
    if size == 0 {
        if is_neg {
            return BigInt::from(-1);
        }

        return BigInt::zero();
    }

    let lw = size / WORD_SIZE_BYTES;
    let mut ws = vec![0usize; lw + 1];
    for i in 0..lw {
        let base = i * WORD_SIZE_BYTES;
        for j in (base..base + WORD_SIZE_BYTES).rev() {
            ws[i] <<= 8;
            ws[i] ^= data[j] as usize;
        }
    }

    for i in (lw * WORD_SIZE_BYTES..size).rev() {
        ws[lw] <<= 8;
        ws[lw] ^= data[i] as usize;
    }

    if is_neg {
        for i in 0..=lw {
            ws[i] = !ws[i];
        }

        let shift = (WORD_SIZE_BYTES - size % WORD_SIZE_BYTES) * 8;
        ws[lw] &= !0usize >> shift;

        n = BigInt::from_signed_bytes_le(&ws.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>());
        n = -n;

        return n - &*BIG_ONE;
    }

    n = BigInt::from_signed_bytes_le(&ws.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>());
    n
}

// getEffectiveSize returns the minimal number of bytes required
// to represent a number (two's complement for negatives).
fn get_effective_size(buf: &[u8], is_neg: bool) -> usize {
    let b = if is_neg { 0xFF } else { 0x00 };

    let mut size = buf.len();
    while size > 0 {
        if buf[size - 1] != b {
            break;
        }
        size -= 1;
    }

    size
}

// ToBytes converts an integer to a slice in little-endian format.
// Note: NEO3 serialization differs from default C# BigInteger.ToByteArray()
// when n == 0. For zero is equal to empty slice in NEO3.
//
// https://github.com/neo-project/neo-vm/blob/master/src/neo-vm/Types/Integer.cs#L16
pub fn to_bytes(n: &BigInt) -> Vec<u8> {
    to_preallocated_bytes(n, &mut vec![])
}

// ToPreallocatedBytes converts an integer to a slice in little-endian format using the given
// byte array for conversion result.
pub fn to_preallocated_bytes(n: &BigInt, data: &mut Vec<u8>) -> Vec<u8> {
    let sign = n.sign();
    if sign == Sign::NoSign {
        return data[..0].to_vec();
    }

    if sign == Sign::Minus {
        let mut bits = n.to_signed_bytes_le();
        let mut carry = true;
        let mut non_zero = false;
        for i in 0..bits.len() {
            if carry {
                bits[i] = bits[i].wrapping_sub(1);
                carry = bits[i] == 0xFF;
            }
            non_zero = non_zero || bits[i] != 0;
        }
        defer! {
            let mut carry = true;
            for i in 0..bits.len() {
                if carry {
                    bits[i] = bits[i].wrapping_add(1);
                    carry = bits[i] == 0;
                } else {
                    break;
                }
            }
        }
        if !non_zero {
            return vec![0xFF];
        }
    }

    let lb = (n.bits() + 7) / 8;

    if data.capacity() < lb {
        *data = vec![0; lb];
    } else {
        data.resize(lb, 0);
    }
    n.to_signed_bytes_le().iter().rev().cloned().collect::<Vec<u8>>().copy_from_slice(data);

    if sign == Sign::Minus {
        for i in 0..data.len() {
            data[i] = !data[i];
        }
    }

    data.to_vec()
}
