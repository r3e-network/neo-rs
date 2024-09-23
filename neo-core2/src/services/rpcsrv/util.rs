use std::error::Error;
use std::fmt;

#[derive(Debug)]
struct RangeError {
    message: String,
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for RangeError {}

fn check_uint32(i: i64) -> Result<(), Box<dyn Error>> {
    if i < 0 || i > u32::MAX as i64 {
        return Err(Box::new(RangeError {
            message: "value should fit uint32".to_string(),
        }));
    }
    Ok(())
}

fn check_int32(i: i64) -> Result<(), Box<dyn Error>> {
    if i < i32::MIN as i64 || i > i32::MAX as i64 {
        return Err(Box::new(RangeError {
            message: "value should fit int32".to_string(),
        }));
    }
    Ok(())
}
