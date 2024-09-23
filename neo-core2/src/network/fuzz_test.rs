use rand::Rng;
use rand::rngs::ThreadRng;
use std::sync::Arc;
use libfuzzer_sys::fuzz_target;
use crate::random;
use crate::io;
use crate::Message;

fuzz_target!(|data: &[u8]| {
    let mut rng = rand::thread_rng();
    for _ in 0..100 {
        let len = rng.gen_range(0..1000);
        let mut seed = vec![0u8; len];
        random::fill(&mut seed);
        // Add seed to the fuzzer input
    }

    let m = Arc::new(Message::default());
    let mut r = io::BinReader::new(data);
    assert!(m.decode(&mut r).is_ok());
});
