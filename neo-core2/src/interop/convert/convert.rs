// Package convert provides functions for type conversion.
mod convert {

    // ToInteger converts its argument to an Integer.
    pub fn to_integer(v: &dyn std::any::Any) -> Option<i32> {
        v.downcast_ref::<i32>().cloned()
    }

    // ToBytes converts its argument to a Buffer VM type.
    pub fn to_bytes(v: &dyn std::any::Any) -> Option<&[u8]> {
        v.downcast_ref::<Vec<u8>>().map(|vec| vec.as_slice())
    }

    // ToString converts its argument to a ByteString VM type.
    pub fn to_string(v: &dyn std::any::Any) -> Option<&str> {
        v.downcast_ref::<String>().map(|s| s.as_str())
    }

    // ToBool converts its argument to a Boolean.
    pub fn to_bool(v: &dyn std::any::Any) -> Option<bool> {
        v.downcast_ref::<bool>().cloned()
    }
}
