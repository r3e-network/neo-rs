//! Transaction signature verification compliance tests

#[cfg(test)]
mod tests {
    use neo_core::cryptography::Secp256r1Crypto;
    use neo_core::network::p2p::helper;
    use neo_core::network::p2p::payloads::Transaction;
    use neo_core::wallets::KeyPair;

    #[test]
    fn test_sign_data_format() {
        let network = 0x4E454F33; // N3 mainnet magic
        let tx = Transaction::default();
        let sign_data = helper::get_sign_data_vec(&tx, network).unwrap();

        // Verify format: 4 bytes network (LE) + 32 bytes hash
        assert_eq!(sign_data.len(), 36);
        assert_eq!(&sign_data[0..4], &network.to_le_bytes());
    }

    #[test]
    fn test_signature_roundtrip() {
        let keypair = KeyPair::generate().unwrap();
        let pubkey = keypair.get_public_key_point().unwrap();
        let pubkey_bytes = pubkey.encode_point(true).unwrap();

        let message = b"test message for signing";
        let signature = keypair.sign(message).unwrap();
        assert_eq!(signature.len(), 64);

        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&signature);
        let verified = Secp256r1Crypto::verify(message, &sig_bytes, &pubkey_bytes).unwrap();
        assert!(verified, "Signature verification should succeed");
    }
}
