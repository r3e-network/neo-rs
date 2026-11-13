use neo_primitives::error::{PrimitiveError, PrimitiveResult};

#[test]
fn primitive_error_invalid_format() {
    let err = PrimitiveError::InvalidFormat {
        message: "bad input".to_string(),
    };
    assert!(err.to_string().contains("bad input"));
}

#[test]
fn primitive_error_invalid_format_constructor() {
    let err = PrimitiveError::invalid_format("test error");
    match err {
        PrimitiveError::InvalidFormat { message } => assert_eq!(message, "test error"),
        _ => panic!("Expected InvalidFormat variant"),
    }
}

#[test]
fn primitive_error_invalid_data() {
    let err = PrimitiveError::InvalidData {
        message: "corrupted data".to_string(),
    };
    assert!(err.to_string().contains("corrupted data"));
}

#[test]
fn primitive_error_invalid_data_constructor() {
    let err = PrimitiveError::invalid_data("invalid bytes");
    match err {
        PrimitiveError::InvalidData { message } => assert_eq!(message, "invalid bytes"),
        _ => panic!("Expected InvalidData variant"),
    }
}

#[test]
fn primitive_error_buffer_overflow() {
    let err = PrimitiveError::BufferOverflow {
        requested: 100,
        available: 50,
    };
    let display = err.to_string();
    assert!(display.contains("100"));
    assert!(display.contains("50"));
}

#[test]
fn primitive_error_buffer_overflow_constructor() {
    let err = PrimitiveError::buffer_overflow(200, 100);
    match err {
        PrimitiveError::BufferOverflow {
            requested,
            available,
        } => {
            assert_eq!(requested, 200);
            assert_eq!(available, 100);
        }
        _ => panic!("Expected BufferOverflow variant"),
    }
}

#[test]
fn primitive_error_type_conversion() {
    let err = PrimitiveError::TypeConversion {
        from: "String".to_string(),
        to: "Integer".to_string(),
    };
    let display = err.to_string();
    assert!(display.contains("String"));
    assert!(display.contains("Integer"));
}

#[test]
fn primitive_error_type_conversion_constructor() {
    let err = PrimitiveError::type_conversion("u64", "u32");
    match err {
        PrimitiveError::TypeConversion { from, to } => {
            assert_eq!(from, "u64");
            assert_eq!(to, "u32");
        }
        _ => panic!("Expected TypeConversion variant"),
    }
}

#[test]
fn primitive_result_ok() {
    let result: PrimitiveResult<i32> = Ok(42);
    assert_eq!(result, Ok(42));
}

#[test]
fn primitive_result_err() {
    let result: PrimitiveResult<i32> = Err(PrimitiveError::invalid_format("test"));
    assert!(result.is_err());
}

#[test]
fn primitive_error_debug_format() {
    let err = PrimitiveError::invalid_format("test");
    let debug = format!("{err:?}");
    assert!(debug.contains("InvalidFormat"));
}

#[test]
fn primitive_error_is_error_trait() {
    let err: Box<dyn std::error::Error> = Box::new(PrimitiveError::invalid_format("test"));
    assert!(err.to_string().contains("test"));
}

#[test]
fn primitive_error_clone() {
    let err = PrimitiveError::invalid_format("original");
    let cloned = err.clone();
    assert_eq!(err, cloned);
}

#[test]
fn primitive_error_equality() {
    let err1 = PrimitiveError::invalid_format("msg");
    let err2 = PrimitiveError::invalid_format("msg");
    let err3 = PrimitiveError::invalid_format("other");

    assert_eq!(err1, err2);
    assert_ne!(err1, err3);
}

#[test]
fn primitive_error_invalid_format_display() {
    let err = PrimitiveError::invalid_format("hex decode failed");
    assert_eq!(err.to_string(), "Invalid format: hex decode failed");
}

#[test]
fn primitive_error_invalid_data_display() {
    let err = PrimitiveError::invalid_data("checksum mismatch");
    assert_eq!(err.to_string(), "Invalid data: checksum mismatch");
}

#[test]
fn primitive_error_buffer_overflow_display() {
    let err = PrimitiveError::buffer_overflow(10, 5);
    assert_eq!(
        err.to_string(),
        "Buffer overflow: attempted to read 10 bytes, but only 5 available"
    );
}

#[test]
fn primitive_error_type_conversion_display() {
    let err = PrimitiveError::type_conversion("Vec<u8>", "u32");
    assert_eq!(
        err.to_string(),
        "Type conversion failed: cannot convert Vec<u8> to u32"
    );
}
