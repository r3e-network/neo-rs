//! Scalar and byte-like RPC conversion implementations.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_serialization::json::{JToken, MAX_SAFE_INTEGER};
use std::str::FromStr;
use uuid::Uuid;

use super::{ConversionContext, RpcConvertible, expect_string, invalid_params, numeric_from_token};
use crate::server::rpc_exception::RpcException;

impl RpcConvertible for String {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        expect_string(token, "Expected string value")
    }
}

impl RpcConvertible for bool {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        Ok(token.as_boolean())
    }
}

macro_rules! impl_numeric_convertible {
    ($($ty:ty),+) => {
        $(
            impl RpcConvertible for $ty {
                fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
                    let value = numeric_from_token(token)?;
                    if value.is_nan() || value.is_infinite() {
                        return Err(invalid_params(format!(
                            "Invalid numeric value: {}",
                            token.to_string_value()
                        )));
                    }

                    let min = <$ty>::MIN as f64;
                    let max = <$ty>::MAX as f64;
                    if value < min || value > max {
                        return Err(invalid_params(format!("Numeric value out of range for {}: {}", stringify!($ty), value)));
                    }

                    if !matches!(stringify!($ty), "f32" | "f64") {
                        let max_safe = MAX_SAFE_INTEGER as f64;
                        if value < -max_safe || value > max_safe {
                            return Err(invalid_params(format!(
                                "Numeric value out of safe range for {}: {}",
                                stringify!($ty),
                                value
                            )));
                        }
                        let rounded = value.round();
                        if (value - rounded).abs() > f64::EPSILON {
                            return Err(invalid_params(format!("Non-integer value for {}: {}", stringify!($ty), value)));
                        }
                        return Ok(rounded as $ty);
                    }

                    Ok(value as $ty)
                }
            }
        )+
    };
}

impl_numeric_convertible!(i8, u8, i16, u16, i32, u32, i64, u64, f64);

impl RpcConvertible for Vec<u8> {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        let text = expect_string(token, "Expected Base64 string")?;
        BASE64_STANDARD
            .decode(text.trim())
            .map_err(|_| invalid_params("Invalid Base64-encoded bytes"))
    }
}

impl RpcConvertible for Uuid {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        let text = expect_string(token, "Expected UUID string")?;
        Self::from_str(text.trim()).map_err(|_| invalid_params(format!("Invalid UUID: {text}")))
    }
}
