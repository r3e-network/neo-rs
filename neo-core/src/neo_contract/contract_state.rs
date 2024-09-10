use std::convert::TryFrom;
use neo_json::jtoken::JToken;
use neo_vm::reference_counter::ReferenceCounter;
use neo_vm::stack_item::StackItem;
use crate::neo_contract::iinteroperable_verifiable::InteroperableVerifiable;
use crate::neo_contract::manifest::contract_manifest::ContractManifest;
use crate::neo_contract::nef_file::NefFile;
use crate::uint160::UInt160;

pub struct ContractState {
    /// The id of the contract.
    pub id: i32,

    /// Indicates the number of times the contract has been updated.
    pub update_counter: u16,

    /// The hash of the contract.
    pub hash: UInt160,

    /// The nef of the contract.
    pub nef: NefFile,

    /// The manifest of the contract.
    pub manifest: ContractManifest,
}

impl ContractState {
    /// The script of the contract.
    pub fn script(&self) -> &[u8] {
        &self.nef.script
    }

    /// Determines whether the current contract has the permission to call the specified contract.
    pub fn can_call(&self, target_contract: &ContractState, target_method: &str) -> bool {
        self.manifest.permissions.iter().any(|u| u.is_allowed(target_contract, target_method))
    }

    /// Converts the contract to a JSON object.
    pub fn to_json(&self) -> JToken::Object {
        let mut json = JObject::new();
        json.insert("id", JValue::from(self.id));
        json.insert("updatecounter", JValue::from(self.update_counter));
        json.insert("hash", JValue::from(self.hash.to_string()));
        json.insert("nef", self.nef.to_json());
        json.insert("manifest", self.manifest.to_json());
        json
    }
}

impl InteroperableVerifiable for ContractState {
    fn from_stack_item(&mut self, stack_item: &StackItem, verify: bool) -> Result<(), Error> {
        if let StackItem::Array(array) = stack_item {
            self.id = i32::try_from(array[0].get_integer()?)?;
            self.update_counter = u16::try_from(array[1].get_integer()?)?;
            self.hash = UInt160::try_from(array[2].get_span()?)?;
            self.nef = NefFile::parse(array[3].get_byte_string()?, verify)?;
            self.manifest = ContractManifest::from_stack_item(&array[4])?;
            Ok(())
        } else {
            Err(Error::InvalidStackItemType)
        }
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        StackItem::Array(vec![
            StackItem::Integer(self.id.into()),
            StackItem::Integer(self.update_counter.into()),
            StackItem::ByteString(self.hash.to_vec()),
            StackItem::ByteString(self.nef.to_vec()),
            self.manifest.to_stack_item(reference_counter),
        ])
    }
}

impl Clone for ContractState {
    fn clone(&self) -> Self {
        ContractState {
            id: self.id,
            update_counter: self.update_counter,
            hash: self.hash,
            nef: self.nef.clone(),
            manifest: self.manifest.clone(),
        }
    }
}
