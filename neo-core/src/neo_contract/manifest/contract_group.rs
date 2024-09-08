
/// Represents a set of mutually trusted contracts.
/// A contract will trust and allow any contract in the same group to invoke it, and the user interface will not give any warnings.
/// A group is identified by a public key and must be accompanied by a signature for the contract hash to prove that the contract is indeed included in the group.
#[derive(Clone, Debug)]
pub struct ContractGroup {
    /// The public key of the group.
    pub pub_key: Secp256r1PublicKey,

    /// The signature of the contract hash which can be verified by `pub_key`.
    pub signature: Vec<u8>,
}

impl ISerializable for ContractGroup {
    fn from_stack_item(stack_item: &StackItem) -> Result<Self, Error> {
        if let StackItem::Struct(s) = stack_item {
            if s.len() != 2 {
                return Err(Error::InvalidStructure);
            }
            Ok(ContractGroup {
                pub_key: Secp256r1PublicKey::decode_point(&s[0].as_bytes()?, Secp256r1)?,
                signature: s[1].as_bytes()?.to_vec(),
            })
        } else {
            Err(Error::InvalidStackItem)
        }
    }

    fn to_stack_item(&self) -> StackItem {
        StackItem::Struct(vec![
            StackItem::ByteArray(self.pub_key.to_array().to_vec()),
            StackItem::ByteArray(self.signature.clone()),
        ])
    }
}

impl ContractGroup {
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
        let pub_key = Secp256r1PublicKey::parse(
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
    pub fn is_valid(&self, hash: &UInt160) -> bool {
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
