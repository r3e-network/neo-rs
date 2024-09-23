use neo_core2::crypto::keys::{KeyPair, PublicKey};
use neo_core2::util::Uint160;
use neo_core2::smartcontract::manifest::{Group, Groups};
use std::convert::TryFrom;

#[cfg(test)]
mod tests {
    use super::*;
    use neo_core2::internal::testserdes;

    #[test]
    fn test_group_json_in_out() {
        let priv_key = KeyPair::new().expect("Failed to create private key");
        let pub_key = priv_key.public_key();
        let sig = vec![0u8; 64]; // Assuming 64-byte signature length
        let g = Group { public_key: pub_key, signature: sig };
        testserdes::marshal_unmarshal_json(&g, &Group::default());
    }

    #[test]
    fn test_groups_are_valid() {
        let mut gps = Groups::new();

        let h = Uint160::try_from([42u8; 20]).unwrap();
        let priv_key = KeyPair::new().expect("Failed to create private key");
        let priv_key2 = KeyPair::new().expect("Failed to create second private key");
        let pub_key = priv_key.public_key();
        let pub_key2 = priv_key2.public_key();
        
        let g_correct = Group {
            public_key: pub_key,
            signature: priv_key.sign(&h.to_be_bytes()).expect("Failed to sign"),
        };
        let g_correct2 = Group {
            public_key: pub_key2,
            signature: priv_key2.sign(&h.to_be_bytes()).expect("Failed to sign"),
        };
        let g_incorrect = Group {
            public_key: pub_key,
            signature: priv_key.sign(&h.to_le_bytes()).expect("Failed to sign"),
        };

        gps = Groups::from(vec![g_correct.clone()]);
        assert!(gps.are_valid(&h).is_ok());

        gps = Groups::from(vec![g_incorrect.clone()]);
        assert!(gps.are_valid(&h).is_err());

        gps = Groups::from(vec![g_correct.clone(), g_correct2]);
        assert!(gps.are_valid(&h).is_ok());

        gps = Groups::from(vec![g_correct.clone(), g_correct]);
        assert!(gps.are_valid(&h).is_err());

        gps = Groups::from(vec![g_incorrect]);
        assert!(gps.are_valid(&Uint160::default()).is_ok()); // empty hash
    }

    #[test]
    fn test_groups_contains() {
        let priv_key = KeyPair::new().expect("Failed to create private key");
        let priv_key2 = KeyPair::new().expect("Failed to create second private key");
        let priv_key3 = KeyPair::new().expect("Failed to create third private key");
        let g1 = Group { public_key: priv_key.public_key(), signature: vec![] };
        let g2 = Group { public_key: priv_key2.public_key(), signature: vec![] };
        let gps = Groups::from(vec![g1, g2]);
        assert!(gps.contains(&priv_key2.public_key()));
        assert!(!gps.contains(&priv_key3.public_key()));
    }
}
