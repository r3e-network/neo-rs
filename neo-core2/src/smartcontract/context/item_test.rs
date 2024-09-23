use std::collections::HashMap;
use crate::crypto::keys::{PrivateKey, PublicKey};
use crate::smartcontract::{Parameter, ParameterType};
use crate::internal::{random, testserdes};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smartcontract::context::Item;

    #[test]
    fn test_context_item_add_signature() {
        let mut item = Item {
            signatures: HashMap::new(),
            ..Default::default()
        };

        let priv1 = PrivateKey::new().expect("Failed to create private key");
        let pub1 = priv1.public_key();
        let sig1 = vec![1, 2, 3];
        item.add_signature(&pub1, sig1.clone());
        assert_eq!(sig1, item.get_signature(&pub1).unwrap());

        let priv2 = PrivateKey::new().expect("Failed to create private key");
        let pub2 = priv2.public_key();
        let sig2 = vec![5, 6, 7];
        item.add_signature(&pub2, sig2.clone());
        assert_eq!(sig2, item.get_signature(&pub2).unwrap());
        assert_eq!(sig1, item.get_signature(&pub1).unwrap());
    }

    #[test]
    fn test_context_item_marshal_json() {
        let priv1 = PrivateKey::new().expect("Failed to create private key");
        let priv2 = PrivateKey::new().expect("Failed to create private key");

        let expected = Item {
            script: Some(vec![1, 2, 3]),
            parameters: vec![Parameter {
                param_type: ParameterType::Signature,
                value: random::bytes(PublicKey::SIGNATURE_LEN),
            }],
            signatures: {
                let mut map = HashMap::new();
                map.insert(priv1.public_key().to_string_compressed(), random::bytes(PublicKey::SIGNATURE_LEN));
                map.insert(priv2.public_key().to_string_compressed(), random::bytes(PublicKey::SIGNATURE_LEN));
                map
            },
        };

        testserdes::marshal_unmarshal_json(&expected, Item::default());

        // Empty script
        let mut expected_empty_script = expected;
        expected_empty_script.script = None;
        testserdes::marshal_unmarshal_json(&expected_empty_script, Item::default());
    }
}
