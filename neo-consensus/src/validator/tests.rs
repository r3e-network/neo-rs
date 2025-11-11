use neo_crypto::{ecc256::PrivateKey, Keypair};

use super::{Validator, ValidatorId, ValidatorSet};
use crate::message::ViewNumber;

#[test]
fn primary_rotates_with_height_and_view() {
    let mut validators = Vec::new();
    for id in 0u16..4 {
        let mut bytes = [0u8; 32];
        bytes[31] = (id + 1) as u8;
        let private = PrivateKey::new(bytes);
        let keypair = Keypair::from_private(private).unwrap();
        validators.push(Validator {
            id: ValidatorId(id),
            public_key: keypair.public_key,
            alias: None,
        });
    }
    let set = ValidatorSet::new(validators);
    let primary0 = set.primary_id(0, ViewNumber::ZERO).unwrap();
    assert_eq!(primary0, ValidatorId(0));
    let primary1 = set.primary_id(1, ViewNumber::ZERO).unwrap();
    assert_eq!(primary1, ValidatorId(1));
    let view1 = set.primary_id(1, ViewNumber(1)).unwrap();
    assert_eq!(view1, ValidatorId(2));
}
