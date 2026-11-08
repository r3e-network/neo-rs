//! Comprehensive tests for Helper module

#[cfg(test)]
mod tests {
    use crate::helper::Helper;
    use neo_crypto::{ECCurve, ECPoint};
    use neo_primitives::UInt160;
    use neo_vm::OpCode;

    #[test]
    fn signature_contract_cost_is_positive() {
        let cost = Helper::signature_contract_cost();
        assert!(cost > 0);
    }

    #[test]
    fn multi_signature_contract_cost_scales_with_m_and_n() {
        let cost_2_of_3 = Helper::multi_signature_contract_cost(2, 3);
        let cost_2_of_5 = Helper::multi_signature_contract_cost(2, 5);
        let cost_3_of_5 = Helper::multi_signature_contract_cost(3, 5);

        assert!(cost_2_of_5 > cost_2_of_3, "More keys should cost more");
        assert!(cost_3_of_5 > cost_2_of_5, "More signatures should cost more");
    }

    #[test]
    fn is_signature_contract_validates_correct_format() {
        let pubkey = ECPoint::random(&ECCurve::Secp256r1).expect("random key");
        let script = Helper::signature_redeem_script(&pubkey.to_bytes());
        assert!(Helper::is_signature_contract(&script));
    }

    #[test]
    fn is_signature_contract_rejects_wrong_length() {
        let short_script = vec![0u8; 39];
        assert!(!Helper::is_signature_contract(&short_script));

        let long_script = vec![0u8; 41];
        assert!(!Helper::is_signature_contract(&long_script));
    }

    #[test]
    fn is_signature_contract_rejects_invalid_pattern() {
        let mut script = vec![0u8; 40];
        script[0] = OpCode::PUSH1.byte(); // Wrong opcode
        assert!(!Helper::is_signature_contract(&script));
    }

    #[test]
    fn is_multi_sig_contract_accepts_valid_script() {
        let keys = vec![
            ECPoint::random(&ECCurve::Secp256r1).expect("key1").to_bytes(),
            ECPoint::random(&ECCurve::Secp256r1).expect("key2").to_bytes(),
            ECPoint::random(&ECCurve::Secp256r1).expect("key3").to_bytes(),
        ];
        let script = Helper::multi_sig_redeem_script(2, &keys);
        assert!(Helper::is_multi_sig_contract(&script));
    }

    #[test]
    fn is_multi_sig_contract_rejects_too_short() {
        let short_script = vec![0u8; 41];
        assert!(!Helper::is_multi_sig_contract(&short_script));
    }

    #[test]
    fn parse_multi_sig_contract_extracts_m_and_keys() {
        let keys = vec![
            ECPoint::random(&ECCurve::Secp256r1).expect("key1").to_bytes(),
            ECPoint::random(&ECCurve::Secp256r1).expect("key2").to_bytes(),
            ECPoint::random(&ECCurve::Secp256r1).expect("key3").to_bytes(),
        ];
        let script = Helper::multi_sig_redeem_script(2, &keys);

        let result = Helper::parse_multi_sig_contract(&script);
        assert!(result.is_some());

        let (m, parsed_keys) = result.unwrap();
        assert_eq!(m, 2);
        assert_eq!(parsed_keys.len(), 3);
    }

    #[test]
    fn parse_multi_sig_contract_returns_none_for_invalid() {
        let invalid_script = vec![0u8; 100];
        assert!(Helper::parse_multi_sig_contract(&invalid_script).is_none());
    }

    #[test]
    fn try_multi_sig_redeem_script_enforces_bounds() {
        let keys = vec![vec![0u8; 33]];

        // m = 0 is invalid
        assert!(Helper::try_multi_sig_redeem_script(0, &keys).is_err());

        // m > n is invalid
        assert!(Helper::try_multi_sig_redeem_script(2, &keys).is_err());
    }

    #[test]
    fn try_multi_sig_redeem_script_rejects_too_many_keys() {
        let keys = vec![vec![0u8; 33]; 1025]; // Exceeds MAX_PUBLIC_KEYS_PER_MULTISIG
        assert!(Helper::try_multi_sig_redeem_script(1, &keys).is_err());
    }

    #[test]
    fn try_multi_sig_redeem_script_validates_key_format() {
        let invalid_key = vec![0xFFu8; 33]; // Not a valid EC point
        let keys = vec![invalid_key];
        assert!(Helper::try_multi_sig_redeem_script(1, &keys).is_err());
    }

    #[test]
    fn is_standard_contract_recognizes_signature() {
        let pubkey = ECPoint::random(&ECCurve::Secp256r1).expect("random key");
        let script = Helper::signature_redeem_script(&pubkey.to_bytes());
        assert!(Helper::is_standard_contract(&script));
    }

    #[test]
    fn is_standard_contract_recognizes_multisig() {
        let keys = vec![
            ECPoint::random(&ECCurve::Secp256r1).expect("key1").to_bytes(),
            ECPoint::random(&ECCurve::Secp256r1).expect("key2").to_bytes(),
        ];
        let script = Helper::multi_sig_redeem_script(1, &keys);
        assert!(Helper::is_standard_contract(&script));
    }

    #[test]
    fn is_standard_contract_rejects_non_standard() {
        let custom_script = vec![OpCode::PUSH1.byte(), OpCode::RET.byte()];
        assert!(!Helper::is_standard_contract(&custom_script));
    }

    #[test]
    fn get_contract_hash_is_deterministic() {
        let sender = UInt160::from_bytes(&[1u8; 20]).expect("sender");
        let hash1 = Helper::get_contract_hash(&sender, 12345, "TestContract");
        let hash2 = Helper::get_contract_hash(&sender, 12345, "TestContract");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn get_contract_hash_varies_with_sender() {
        let sender1 = UInt160::from_bytes(&[1u8; 20]).expect("sender1");
        let sender2 = UInt160::from_bytes(&[2u8; 20]).expect("sender2");
        let hash1 = Helper::get_contract_hash(&sender1, 12345, "TestContract");
        let hash2 = Helper::get_contract_hash(&sender2, 12345, "TestContract");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn get_contract_hash_varies_with_checksum() {
        let sender = UInt160::from_bytes(&[1u8; 20]).expect("sender");
        let hash1 = Helper::get_contract_hash(&sender, 12345, "TestContract");
        let hash2 = Helper::get_contract_hash(&sender, 54321, "TestContract");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn get_contract_hash_varies_with_name() {
        let sender = UInt160::from_bytes(&[1u8; 20]).expect("sender");
        let hash1 = Helper::get_contract_hash(&sender, 12345, "TestContract");
        let hash2 = Helper::get_contract_hash(&sender, 12345, "OtherContract");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn parse_multi_sig_invocation_valid_signatures() {
        let sig1 = vec![0xAA; 64];
        let sig2 = vec![0xBB; 64];

        let mut invocation = Vec::new();
        invocation.push(OpCode::PUSHDATA1.byte());
        invocation.push(64);
        invocation.extend_from_slice(&sig1);
        invocation.push(OpCode::PUSHDATA1.byte());
        invocation.push(64);
        invocation.extend_from_slice(&sig2);

        let result = Helper::parse_multi_sig_invocation(&invocation, 2);
        assert!(result.is_some());

        let signatures = result.unwrap();
        assert_eq!(signatures.len(), 2);
        assert_eq!(signatures[0], sig1);
        assert_eq!(signatures[1], sig2);
    }

    #[test]
    fn parse_multi_sig_invocation_rejects_wrong_count() {
        let sig = vec![0xAA; 64];

        let mut invocation = Vec::new();
        invocation.push(OpCode::PUSHDATA1.byte());
        invocation.push(64);
        invocation.extend_from_slice(&sig);

        // Expects 2 signatures but only 1 provided
        let result = Helper::parse_multi_sig_invocation(&invocation, 2);
        assert!(result.is_none());
    }

    #[test]
    fn parse_multi_sig_invocation_rejects_zero_required() {
        let invocation = vec![];
        assert!(Helper::parse_multi_sig_invocation(&invocation, 0).is_none());
    }

    #[test]
    fn parse_multi_sig_invocation_rejects_wrong_opcode() {
        let mut invocation = Vec::new();
        invocation.push(OpCode::PUSH1.byte()); // Wrong opcode
        invocation.push(64);
        invocation.extend_from_slice(&vec![0xAA; 64]);

        assert!(Helper::parse_multi_sig_invocation(&invocation, 1).is_none());
    }

    #[test]
    fn parse_multi_sig_invocation_rejects_wrong_sig_length() {
        let mut invocation = Vec::new();
        invocation.push(OpCode::PUSHDATA1.byte());
        invocation.push(63); // Wrong length (not 64)
        invocation.extend_from_slice(&vec![0xAA; 63]);

        assert!(Helper::parse_multi_sig_invocation(&invocation, 1).is_none());
    }

    #[test]
    fn max_verification_gas_constant() {
        assert_eq!(Helper::MAX_VERIFICATION_GAS, 150_000_000);
    }

    #[test]
    fn max_public_keys_per_multisig_constant() {
        assert_eq!(Helper::MAX_PUBLIC_KEYS_PER_MULTISIG, 1024);
    }

    #[test]
    fn signature_redeem_script_has_correct_structure() {
        let pubkey = ECPoint::random(&ECCurve::Secp256r1).expect("key");
        let script = Helper::signature_redeem_script(&pubkey.to_bytes());

        assert_eq!(script.len(), 40);
        assert_eq!(script[0], OpCode::PUSHDATA1.byte());
        assert_eq!(script[1], 33); // pubkey length
        assert_eq!(script[35], OpCode::SYSCALL.byte());
    }

    #[test]
    fn multi_sig_with_single_key() {
        let keys = vec![
            ECPoint::random(&ECCurve::Secp256r1).expect("key").to_bytes(),
        ];
        let script = Helper::multi_sig_redeem_script(1, &keys);

        let parsed = Helper::parse_multi_sig_contract(&script);
        assert!(parsed.is_some());
        let (m, parsed_keys) = parsed.unwrap();
        assert_eq!(m, 1);
        assert_eq!(parsed_keys.len(), 1);
    }

    #[test]
    fn multi_sig_with_max_keys() {
        // Test with a reasonable number (not full 1024 due to test performance)
        let key_count = 10;
        let keys: Vec<Vec<u8>> = (0..key_count)
            .map(|_| ECPoint::random(&ECCurve::Secp256r1).expect("key").to_bytes())
            .collect();

        let script = Helper::multi_sig_redeem_script(key_count / 2, &keys);

        let parsed = Helper::parse_multi_sig_contract(&script);
        assert!(parsed.is_some());
        let (m, parsed_keys) = parsed.unwrap();
        assert_eq!(m, key_count / 2);
        assert_eq!(parsed_keys.len(), key_count);
    }
}
