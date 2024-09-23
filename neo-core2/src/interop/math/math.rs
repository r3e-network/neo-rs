/// Package math provides access to useful numeric functions available in Neo VM.
mod math {
    use crate::interop::neogointernal;

    /// Pow returns a^b using POW VM opcode.
    /// b must be >= 0 and <= 2^31-1.
    pub fn pow(a: i32, b: i32) -> i32 {
        neogointernal::opcode2("POW", a, b) as i32
    }

    /// Sqrt returns a positive square root of x rounded down.
    pub fn sqrt(x: i32) -> i32 {
        neogointernal::opcode1("SQRT", x) as i32
    }

    /// Sign returns:
    ///
    /// -1 if x <  0
    ///  0 if x == 0
    /// +1 if x >  0
    pub fn sign(a: i32) -> i32 {
        neogointernal::opcode1("SIGN", a) as i32
    }

    /// Abs returns an absolute value of a.
    pub fn abs(a: i32) -> i32 {
        neogointernal::opcode1("ABS", a) as i32
    }

    /// Max returns the maximum of a, b.
    pub fn max(a: i32, b: i32) -> i32 {
        neogointernal::opcode2("MAX", a, b) as i32
    }

    /// Min returns the minimum of a, b.
    pub fn min(a: i32, b: i32) -> i32 {
        neogointernal::opcode2("MIN", a, b) as i32
    }

    /// Within returns true if a <= x < b.
    pub fn within(x: i32, a: i32, b: i32) -> bool {
        neogointernal::opcode3("WITHIN", x, a, b) as bool
    }

    /// ModMul returns the result of modulus division on a*b.
    pub fn mod_mul(a: i32, b: i32, mod_: i32) -> i32 {
        neogointernal::opcode3("MODMUL", a, b, mod_) as i32
    }

    /// ModPow returns the result of modulus division on a^b. If b is -1,
    /// it returns the modular inverse of a.
    pub fn mod_pow(a: i32, b: i32, mod_: i32) -> i32 {
        neogointernal::opcode3("MODPOW", a, b, mod_) as i32
    }
}
