use alloc::rc::Rc;
use NeoRust::crypto::ECPoint;
use neo_base::encoding::base64;
use neo_vm::stack_item::StackItem;
use neo_vm::References;
use neo_vm::StackItem;
use crate::{io::iserializable::ISerializable, neo_contract::iinteroperable::IInteroperable};
use crate::cryptography::ECPoint;
use crate::neo_contract::manifest::manifest_error::ManifestError;

/// Represents a set of mutually trusted contracts.
/// A contract will trust and allow any contract in the same group to invoke it, and the user interface will not give any warnings.
/// A group is identified by a public key and must be accompanied by a signature for the contract hash to prove that the contract is indeed included in the group.
#[derive(Clone, Debug)]
pub struct ContractGroup {
    /// The public key of the group.
    pub pub_key: ECPoint,

    /// The signature of the contract hash which can be verified by `pub_key`.
    pub signature: Vec<u8>,
}

impl Default for ContractGroup {
    fn default() -> Self {
        todo!()
    }
}

impl IInteroperable for ContractGroup {
    type Error = ManifestError;

    fn from_stack_item(stack_item: &Rc<StackItem>) -> Result<Self, Self::Error> {
        if let StackItem::Struct(s) = stack_item {
            if s.len() != 2 {
                return Err(Self::Error::InvalidStructure);
            }
            Ok(ContractGroup {
                pub_key: ECPoint::decode_point(&s[0].as_bytes()?, Secp256r1)?,
                signature: s[1].as_bytes()?.to_vec(),
            })
        } else {
            Err(Self::Error::InvalidStackItem)
        }
    }

    fn to_stack_item(&self, reference_counter: &mut References) -> Result<Rc<StackItem>, Self::Error> {
        Ok(StackItem::Struct(Struct::new(vec![
            StackItem::ByteArray(self.pub_key.to_array().to_vec()),
            StackItem::ByteArray(self.signature.clone()),
        ])))
    }
}

impl  ContractGroup {
    /// Converts the group from a JSON object.
    ///
    /// # Arguments
    ///
    /// * `json` - The group represented by a JSON object.
    ///
    /// # Returns
    ///
    /// The converted group.
    pub fn from_json(json: &JsonValue) -> Result<Self, Error> {
        let pub_key = ECPoint::parse(
            json["pubkey"].as_str().ok_or(Error::InvalidFormat)?,
            Secp256r1,
        )?;
        let signature = base64::decode(json["signature"].as_str().ok_or(Error::InvalidFormat)?)?;
        
        if signature.len() != 64 {
            return Err(Error::InvalidFormat);
        }

        Ok(ContractGroup {
            pub_key,
            signature,
        })
    }

    /// Determines whether the signature in the group is valid.
    ///
    /// # Arguments
    ///
    /// * `hash` - The hash of the contract.
    ///
    /// # Returns
    ///
    /// `true` if the signature is valid; otherwise, `false`.
    pub fn is_valid(&self, hash: &H160) -> bool {
        crypto::verify_signature(&hash.to_array(), &self.signature, &self.pub_key)
    }

    /// Converts the group to a JSON object.
    ///
    /// # Returns
    ///
    /// The group represented by a JSON object.
    pub fn to_json(&self) -> JsonValue {
        json!({
            "pubkey": self.pub_key.to_string(),
            "signature": base64::encode(&self.signature),
        })
    }
}
