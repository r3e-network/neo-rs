use std::cmp::Ordering;
use std::convert::TryInto;
use std::io::Read;
use std::sync::Once;
use rand::Rng;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use libfuzzer_sys::fuzz_target;

static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
        // Initialization code here
    });
}

fn from_bytes(bytes: &[u8]) -> BigInt {
    BigInt::from_signed_bytes_be(bytes)
}

fn to_bytes(bi: &BigInt) -> Vec<u8> {
    bi.to_signed_bytes_be()
}

fuzz_target!(|data: &[u8]| {
    setup();

    let mut rng = rand::thread_rng();
    let mut test_cases = vec![];

    for _ in 0..50 {
        for j in 1..MaxBytesLen {
            let mut b = vec![0u8; j];
            rng.fill(&mut b[..]);
            test_cases.push(b);
        }
    }

    for tc in test_cases {
        let bi = from_bytes(&tc);
        let actual = to_bytes(&bi);
        assert!(actual.len() <= tc.len(), "actual: {:x?}, raw: {:x?}", actual, tc);

        assert!(actual == &tc[..actual.len()], "actual: {:x?}, raw: {:x?}", actual, tc);
        if actual.len() == tc.len() {
            continue;
        }

        let mut b = 0u8;
        if bi.sign() == Ordering::Less {
            b = 0xFF;
        }
        for i in actual.len()..tc.len() {
            assert_eq!(b, tc[i], "invalid prefix");
        }

        let new_raw = to_bytes(&bi);
        let new_bi = from_bytes(&new_raw);
        assert_eq!(bi, new_bi);
    }
});
