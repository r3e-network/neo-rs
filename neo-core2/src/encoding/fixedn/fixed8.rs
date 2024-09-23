use std::cmp;
use std::fmt;
use std::str::FromStr;

const PRECISION: i32 = 8;
const DECIMALS: i64 = 100000000;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct Fixed8(i64);

impl fmt::Display for Fixed8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut val = self.0;
        if val < 0 {
            write!(f, "-")?;
            val = -val;
        }
        let mut str = format!("{}", val / DECIMALS);
        write!(f, "{}", str)?;
        val %= DECIMALS;
        if val > 0 {
            write!(f, ".")?;
            str = format!("{}", val);
            for _ in str.len()..8 {
                write!(f, "0")?;
            }
            write!(f, "{}", str.trim_end_matches('0'))?;
        }
        Ok(())
    }
}

impl Fixed8 {
    pub fn float_value(&self) -> f64 {
        self.0 as f64 / DECIMALS as f64
    }

    pub fn integral_value(&self) -> i64 {
        self.0 / DECIMALS
    }

    pub fn fractional_value(&self) -> i32 {
        (self.0 % DECIMALS) as i32
    }

    pub fn from_int64(val: i64) -> Self {
        Fixed8(DECIMALS * val)
    }

    pub fn from_float(val: f64) -> Self {
        Fixed8((DECIMALS as f64 * val) as i64)
    }

    pub fn from_string(s: &str) -> Result<Self, std::num::ParseIntError> {
        let num = i64::from_str(s)?;
        Ok(Fixed8(num * DECIMALS))
    }

    pub fn unmarshal_json(data: &[u8]) -> Result<Self, std::num::ParseIntError> {
        if data.len() > 2 && data[0] == b'"' && data[data.len() - 1] == b'"' {
            let data = &data[1..data.len() - 1];
            Self::from_string(std::str::from_utf8(data).unwrap())
        } else {
            Self::from_string(std::str::from_utf8(data).unwrap())
        }
    }

    pub fn unmarshal_yaml(unmarshal: &dyn Fn(&mut String) -> Result<(), std::num::ParseIntError>) -> Result<Self, std::num::ParseIntError> {
        let mut s = String::new();
        unmarshal(&mut s)?;
        Self::from_string(&s)
    }

    fn set_from_string(&mut self, s: &str) -> Result<(), std::num::ParseIntError> {
        let p = Self::from_string(s)?;
        *self = p;
        Ok(())
    }

    pub fn marshal_json(&self) -> Result<Vec<u8>, std::num::ParseIntError> {
        Ok(format!("\"{}\"", self).into_bytes())
    }

    pub fn marshal_yaml(&self) -> Result<String, std::num::ParseIntError> {
        Ok(self.to_string())
    }

    pub fn decode_binary(r: &mut dyn io::Read) -> Result<Self, std::io::Error> {
        let mut buf = [0u8; 8];
        r.read_exact(&mut buf)?;
        Ok(Fixed8(i64::from_le_bytes(buf)))
    }

    pub fn encode_binary(&self, w: &mut dyn io::Write) -> Result<(), std::io::Error> {
        w.write_all(&self.0.to_le_bytes())
    }

    pub fn satoshi() -> Self {
        Fixed8(1)
    }

    pub fn div(&self, i: i64) -> Self {
        Fixed8(self.0 / i)
    }

    pub fn add(&self, g: Fixed8) -> Self {
        Fixed8(self.0 + g.0)
    }

    pub fn sub(&self, g: Fixed8) -> Self {
        Fixed8(self.0 - g.0)
    }

    pub fn less_than(&self, g: Fixed8) -> bool {
        self.0 < g.0
    }

    pub fn greater_than(&self, g: Fixed8) -> bool {
        self.0 > g.0
    }

    pub fn equal(&self, g: Fixed8) -> bool {
        self.0 == g.0
    }

    pub fn compare(&self, g: Fixed8) -> cmp::Ordering {
        self.0.cmp(&g.0)
    }
}
