#[cfg(test)]
mod tests {
    use super::*;
    use crate::testserdes;
    use assert_matches::assert_matches;

    #[test]
    fn test_encode_decode_binary() {
        let payload = Ping::new(1, 2);
        assert_ne!(0, payload.timestamp);

        let mut decoded_ping = Ping::default();
        testserdes::encode_decode_binary(&payload, &mut decoded_ping);

        assert_eq!(1, decoded_ping.last_block_index);
        assert_eq!(2, decoded_ping.nonce);
    }
}
