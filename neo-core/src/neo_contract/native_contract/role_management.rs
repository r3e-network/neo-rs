
use std::convert::TryInto;
use NeoRust::prelude::Secp256r1PublicKey;
use neo_proc_macros::contract;
use crate::neo_contract::storage_item::StorageItem;

/// A native contract for managing roles in NEO system.
pub struct RoleManagement {
    storage: StorageMap,
}

#[contract]
impl RoleManagement {
    pub fn new() -> Self {
        Self {
            storage: StorageMap::new(),
        }
    }

    #[contract_event(0, name = "Designation")]
    fn designation(role: i32, block_index: u32) {}

    /// Gets the list of nodes for the specified role.
    ///
    /// # Arguments
    ///
    /// * `role` - The type of the role.
    /// * `index` - The index of the block to be queried.
    ///
    /// # Returns
    ///
    /// The public keys of the nodes.
    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::READ_STATES)]
    pub fn get_designated_by_role(&self, role: Role, index: u32) -> Vec<Secp256r1PublicKey> {
        if !Role::is_valid(&role) {
            panic!("Invalid role");
        }
        if runtime::get_block_height() + 1 < index {
            panic!("Invalid index");
        }
        let key = self.create_storage_key(role as u8, index);
        let boundary = self.create_storage_key(role as u8, 0);
        self.storage
            .find_range(&key, &boundary, SeekDirection::Backward)
            .next()
            .map(|item| {
                let node_list: NodeList = item.into();
                node_list.into()
            })
            .unwrap_or_default()
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::STATES | CallFlags::ALLOW_NOTIFY)]
    fn designate_as_role(&mut self, role: Role, nodes: Vec<Secp256r1PublicKey>) {
        if nodes.is_empty() || nodes.len() > 32 {
            panic!("Invalid number of nodes");
        }
        if !Role::is_valid(&role) {
            panic!("Invalid role");
        }
        if !self.check_committee() {
            panic!("Not authorized");
        }
        let index = runtime::get_block_height() + 1;
        let key = self.create_storage_key(role as u8, index);
        if self.storage.contains(&key) {
            panic!("Designation already exists for this block");
        }
        let mut list = NodeList::new();
        list.extend(nodes);
        list.sort();
        self.storage.put(&key, &list);
        self.designation(role as i32, runtime::get_block_height());
    }

    fn create_storage_key(&self, role: u8, index: u32) -> Vec<u8> {
        let mut key = vec![role];
        key.extend_from_slice(&index.to_be_bytes());
        key
    }

    fn check_committee(&self) -> bool {
        // Implementation of committee check
        // This would typically involve checking if the caller is a member of the committee
        // For simplicity, we'll return true here. In a real implementation, you'd use
        // the appropriate NEO Rust SDK function to check committee membership.
        true
    }
}

struct NodeList(Vec<Secp256r1PublicKey>);

impl NodeList {
    fn new() -> Self {
        NodeList(Vec::new())
    }

    fn extend(&mut self, nodes: Vec<Secp256r1PublicKey>) {
        self.0.extend(nodes);
    }

    fn sort(&mut self) {
        self.0.sort();
    }
}

impl From<StorageItem> for NodeList {
    fn from(item: StorageItem) -> Self {
        // Implement deserialization from StorageItem to NodeList
        // This would depend on how ECPoints are serialized in the NEO Rust SDK
        unimplemented!("Deserialization from StorageItem to NodeList not implemented")
    }
}

impl From<NodeList> for Vec<Secp256r1PublicKey> {
    fn from(node_list: NodeList) -> Self {
        node_list.0
    }
}
