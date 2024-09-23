use neo_core2::crypto::keys::{PublicKey, PrivateKey};
use neo_core2::io::BinReader;
use neo_core2::vm::opcode::Opcode;
use neo_core2::interop::interopnames;
use neo_core2::smartcontract::contract::{create_multi_sig_redeem_script, create_default_multi_sig_redeem_script, create_majority_multi_sig_redeem_script};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_create_multi_sig_redeem_script() {
        let val1 = PublicKey::from_string("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c").unwrap();
        let val2 = PublicKey::from_string("02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093").unwrap();
        let val3 = PublicKey::from_string("03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a").unwrap();

        let validators = vec![val1, val2, val3];

        let out = create_multi_sig_redeem_script(3, &validators).unwrap();

        let mut br = BinReader::new(Cursor::new(&out));
        assert_eq!(Opcode::PUSH3, Opcode::from(br.read_u8().unwrap()));

        for validator in &validators {
            assert_eq!(Opcode::PUSHDATA1, Opcode::from(br.read_u8().unwrap()));
            let bb = br.read_var_bytes().unwrap();
            assert_eq!(validator.to_bytes(), bb);
        }

        assert_eq!(Opcode::PUSH3, Opcode::from(br.read_u8().unwrap()));
        assert_eq!(Opcode::SYSCALL, Opcode::from(br.read_u8().unwrap()));
        assert_eq!(interopnames::to_id(interopnames::SYSTEM_CRYPTO_CHECK_MULTISIG.as_bytes()), br.read_u32_le().unwrap());
    }

    #[test]
    fn test_create_default_multi_sig_redeem_script() {
        let mut validators = Vec::new();

        let add_key = || {
            let key = PrivateKey::new().unwrap();
            validators.push(key.public_key());
        };

        let check_m = |m: usize| {
            let valid_script = create_multi_sig_redeem_script(m, &validators).unwrap();
            let default_script = create_default_multi_sig_redeem_script(&validators).unwrap();
            assert_eq!(valid_script, default_script);
        };

        // 1 out of 1
        add_key();
        check_m(1);

        // 2 out of 2
        add_key();
        check_m(2);

        // 3 out of 4
        for _ in 0..2 {
            add_key();
        }
        check_m(3);

        // 5 out of 6
        for _ in 0..2 {
            add_key();
        }
        check_m(5);

        // 5 out of 7
        add_key();
        check_m(5);

        // 7 out of 10
        for _ in 0..3 {
            add_key();
        }
        check_m(7);
    }

    #[test]
    fn test_create_majority_multi_sig_redeem_script() {
        let mut validators = Vec::new();

        let add_key = || {
            let key = PrivateKey::new().unwrap();
            validators.push(key.public_key());
        };

        let check_m = |m: usize| {
            let valid_script = create_multi_sig_redeem_script(m, &validators).unwrap();
            let majority_script = create_majority_multi_sig_redeem_script(&validators).unwrap();
            assert_eq!(valid_script, majority_script);
        };

        // 1 out of 1
        add_key();
        check_m(1);

        // 2 out of 2
        add_key();
        check_m(2);

        // 3 out of 4
        add_key();
        add_key();
        check_m(3);

        // 4 out of 6
        add_key();
        add_key();
        check_m(4);

        // 5 out of 8
        add_key();
        add_key();
        check_m(5);

        // 6 out of 10
        add_key();
        add_key();
        check_m(6);
    }
}
