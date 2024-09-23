use std::error::Error;
use std::fmt;
use std::str::FromStr;
use num_bigint::BigInt;
use num_traits::{Zero, One};

const MAX_ALLOWED_PRECISION: usize = 16;

// ErrInvalidFormat is returned when decimal format is invalid.
#[derive(Debug, Clone)]
struct ErrInvalidFormat;

impl fmt::Display for ErrInvalidFormat {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid decimal format")
    }
}

impl Error for ErrInvalidFormat {}

lazy_static::lazy_static! {
    static ref POW10: Vec<BigInt> = {
        let mut pow10 = Vec::new();
        let mut p = BigInt::one();
        for _ in 0..=MAX_ALLOWED_PRECISION {
            pow10.push(p.clone());
            p *= 10;
        }
        pow10
    };
}

fn pow10(n: usize) -> BigInt {
    if n <= MAX_ALLOWED_PRECISION {
        POW10[n].clone()
    } else {
        let mut p = POW10[MAX_ALLOWED_PRECISION].clone();
        for _ in MAX_ALLOWED_PRECISION..n {
            p *= 10;
        }
        p
    }
}

// ToString converts a big decimal with the specified precision to a string.
fn to_string(bi: &BigInt, precision: usize) -> String {
    let (dp, fp) = bi.div_rem(&pow10(precision));
    let mut s = dp.to_string();
    if fp.is_zero() {
        return s;
    }
    let mut frac = fp.clone();
    let mut trimmed = 0;
    while (&frac % 10).is_zero() {
        frac /= 10;
        trimmed += 1;
    }
    s.push('.');
    s.push_str(&format!("{:0width$}", frac, width = precision - trimmed));
    s
}

// FromString converts a string to a big decimal with the specified precision.
fn from_string(s: &str, precision: usize) -> Result<BigInt, Box<dyn Error>> {
    let parts: Vec<&str> = s.splitn(2, '.').collect();
    let mut bi = BigInt::from_str(parts[0])?;
    bi *= pow10(precision);
    if parts.len() == 1 {
        return Ok(bi);
    }

    if parts[1].len() > precision {
        return Err(Box::new(ErrInvalidFormat));
    }
    let mut fp = BigInt::from_str(parts[1])?;
    fp *= pow10(precision - parts[1].len());
    if bi.sign() == num_bigint::Sign::Minus {
        bi -= fp;
    } else {
        bi += fp;
    }
    Ok(bi)
}
