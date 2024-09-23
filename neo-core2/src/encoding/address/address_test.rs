use std::str::FromStr;
use neo_core2::util::Uint160;
use neo_core2::encoding::address::{StringToUint160, Uint160ToString};
use assert2::{assert, check};
use anyhow::Result;

#[test]
fn test_uint160_decode_encode_address() -> Result<()> {
    let addrs = vec![
        "NRHkiY2hLy5ypD32CKZtL6pNwhbFMqDEhR",
        "NPCD6gAxNuuJqssZY1eCJabuaz4BjBUHab",
        "NUJUhgvvQyp6AmDBg3QRQ1cmRkMRhaXqZP",
    ];
    for addr in addrs {
        let val = StringToUint160(addr)?;
        check!(addr == Uint160ToString(&val));
    }
    Ok(())
}

#[test]
fn test_uint160_decode_known_address() -> Result<()> {
    let address = "NNnFn8iHWWnJe9QYoN1r4PeXMuVpfLVRS7";

    let val = StringToUint160(address)?;
    check!(val.to_string_le() == "b28427088a3729b2536d10122960394e8be6721f");
    check!(val.to_string() == "1f72e68b4e39602912106d53b229378a082784b2");
    Ok(())
}

#[test]
fn test_uint160_decode_bad_base58() {
    let address = "AJeAEsmeD6t279Dx4n2HWdUvUmmXQ4iJv@";

    let result = StringToUint160(address);
    assert!(result.is_err());
}

#[test]
fn test_uint160_decode_bad_prefix() {
    // The same AJeAEsmeD6t279Dx4n2HWdUvUmmXQ4iJvP key encoded with 0x18 prefix.
    let address = "AhymDz4vvHLtvaN36CMbzkki7H2U8ENb8F";

    let result = StringToUint160(address);
    assert!(result.is_err());
}

#[test]
fn test_prefix_first_letter() {
    let mut u = Uint160::default();
    check!(Uint160ToString(&u).chars().next().unwrap() == 'N');

    for i in 0..u.len() {
        u[i] = 0xFF;
    }
    check!(Uint160ToString(&u).chars().next().unwrap() == 'N');
}
