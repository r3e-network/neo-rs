//! Binary wire serialization for `ContractManifest`.

use neo_error::{CoreError, CoreResult};
use neo_io::serializable::helper::SerializeHelper;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::constants::{MAX_SCRIPT_LENGTH, MAX_SCRIPT_SIZE};

use crate::manifest::{
    ContractAbi, ContractGroup, ContractManifest, ContractPermission, ContractPermissionDescriptor,
    WildCardContainer,
};

fn map_json_error(err: serde_json::Error) -> IoError {
    IoError::invalid_data(err.to_string())
}

fn write_json_item<T: serde::Serialize>(item: &T, writer: &mut BinaryWriter) -> IoResult<()> {
    let json = serde_json::to_string(item).map_err(map_json_error)?;
    writer.write_var_string(&json)
}

fn read_json_item<T: for<'de> serde::Deserialize<'de>>(
    reader: &mut MemoryReader,
    max_len: usize,
) -> IoResult<T> {
    let json = reader.read_var_string(max_len)?;
    serde_json::from_str(&json).map_err(map_json_error)
}

impl ContractManifest {
    /// Gets the size of the manifest in bytes.
    pub fn size(&self) -> usize {
        // NOTE: The binary manifest format embeds several fields as JSON strings (matching the C#
        // node). Size calculations must mirror `serialize_io` exactly.
        let mut size = 0usize;

        size += SerializeHelper::get_var_size_str(&self.name);

        size += SerializeHelper::get_var_size(self.groups.len() as u64);
        for group in &self.groups {
            let group_json = serde_json::to_string(group).unwrap_or_default();
            size += SerializeHelper::get_var_size_str(&group_json);
        }

        let features_json = serde_json::to_string(&self.features).unwrap_or_default();
        size += SerializeHelper::get_var_size_str(&features_json);

        size += SerializeHelper::get_var_size(self.supported_standards.len() as u64);
        for standard in &self.supported_standards {
            size += SerializeHelper::get_var_size_str(standard);
        }

        let abi_json = serde_json::to_string(&self.abi).unwrap_or_default();
        size += SerializeHelper::get_var_size_str(&abi_json);

        size += SerializeHelper::get_var_size(self.permissions.len() as u64);
        for permission in &self.permissions {
            let permission_json = serde_json::to_string(permission).unwrap_or_default();
            size += SerializeHelper::get_var_size_str(&permission_json);
        }

        match &self.trusts {
            WildCardContainer::Wildcard => size += SerializeHelper::get_var_size(0),
            WildCardContainer::List(trusts) => {
                size += SerializeHelper::get_var_size(trusts.len() as u64);
                for trust in trusts {
                    let trust_json = serde_json::to_string(trust).unwrap_or_default();
                    size += SerializeHelper::get_var_size_str(&trust_json);
                }
            }
        }

        let extra_json = self
            .extra
            .as_ref()
            .and_then(|value| serde_json::to_string(value).ok())
            .unwrap_or_default();
        size += SerializeHelper::get_var_size_str(&extra_json);

        size
    }

    /// Serializes the contract manifest to bytes.
    pub fn serialize(&self, writer: &mut BinaryWriter) -> CoreResult<()> {
        self.serialize_io(writer)
            .map_err(|e| CoreError::serialization(e.to_string()))
    }

    fn serialize_io(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_var_string(&self.name)?;

        writer.write_var_int(self.groups.len() as u64)?;
        for group in &self.groups {
            self.serialize_contract_group(group, writer)?;
        }

        let features_json = serde_json::to_string(&self.features).map_err(map_json_error)?;
        writer.write_var_string(&features_json)?;

        writer.write_var_int(self.supported_standards.len() as u64)?;
        for standard in &self.supported_standards {
            writer.write_var_string(standard)?;
        }

        self.serialize_contract_abi(&self.abi, writer)?;

        writer.write_var_int(self.permissions.len() as u64)?;
        for permission in &self.permissions {
            self.serialize_contract_permission(permission, writer)?;
        }

        match &self.trusts {
            WildCardContainer::Wildcard => writer.write_var_int(0)?,
            WildCardContainer::List(trusts) => {
                writer.write_var_int(trusts.len() as u64)?;
                for trust in trusts {
                    let trust_json = serde_json::to_string(trust).map_err(map_json_error)?;
                    writer.write_var_string(&trust_json)?;
                }
            }
        }

        let extra_json = match &self.extra {
            Some(value) => serde_json::to_string(value).map_err(map_json_error)?,
            None => String::new(),
        };
        writer.write_var_string(&extra_json)?;

        Ok(())
    }

    /// Deserializes the contract manifest from bytes.
    pub fn deserialize(reader: &mut MemoryReader) -> CoreResult<Self> {
        Self::deserialize_io(reader).map_err(|e| CoreError::serialization(e.to_string()))
    }

    fn deserialize_io(reader: &mut MemoryReader) -> IoResult<Self> {
        let name = reader.read_var_string(MAX_SCRIPT_SIZE)?;

        let groups_count = reader.read_var_int(256)? as usize;
        let mut groups = Vec::with_capacity(groups_count);
        for _ in 0..groups_count {
            let group = Self::deserialize_contract_group(reader)?;
            groups.push(group);
        }

        let features_json = reader.read_var_string(MAX_SCRIPT_LENGTH)?;
        let features = serde_json::from_str(&features_json).map_err(map_json_error)?;

        let standards_count = reader.read_var_int(256)? as usize;
        let mut supported_standards = Vec::with_capacity(standards_count);
        for _ in 0..standards_count {
            let standard = reader.read_var_string(256)?;
            supported_standards.push(standard);
        }

        let abi = Self::deserialize_contract_abi(reader)?;

        let permissions_count = reader.read_var_int(256)? as usize;
        let mut permissions = Vec::with_capacity(permissions_count);
        for _ in 0..permissions_count {
            let permission = Self::deserialize_contract_permission(reader)?;
            permissions.push(permission);
        }

        let trusts_count = reader.read_var_int(256)? as usize;
        let trusts = if trusts_count == 0 {
            WildCardContainer::create_wildcard()
        } else {
            let mut entries = Vec::with_capacity(trusts_count);
            for _ in 0..trusts_count {
                let trust_json = reader.read_var_string(MAX_SCRIPT_SIZE)?;
                let trust: ContractPermissionDescriptor =
                    serde_json::from_str(&trust_json).map_err(map_json_error)?;
                entries.push(trust);
            }
            WildCardContainer::create(entries)
        };

        let extra_json = reader.read_var_string(MAX_SCRIPT_LENGTH)?;
        let extra = if extra_json.is_empty() {
            None
        } else {
            Some(serde_json::from_str(&extra_json).map_err(map_json_error)?)
        };

        Ok(Self {
            name,
            groups,
            features,
            supported_standards,
            abi,
            permissions,
            trusts,
            extra,
        })
    }

    fn serialize_contract_group(
        &self,
        group: &ContractGroup,
        writer: &mut BinaryWriter,
    ) -> IoResult<()> {
        write_json_item(group, writer)
    }

    fn deserialize_contract_group(reader: &mut MemoryReader) -> IoResult<ContractGroup> {
        read_json_item(reader, MAX_SCRIPT_SIZE)
    }

    fn serialize_contract_abi(&self, abi: &ContractAbi, writer: &mut BinaryWriter) -> IoResult<()> {
        write_json_item(abi, writer)
    }

    fn deserialize_contract_abi(reader: &mut MemoryReader) -> IoResult<ContractAbi> {
        read_json_item(reader, MAX_SCRIPT_LENGTH)
    }

    fn serialize_contract_permission(
        &self,
        permission: &ContractPermission,
        writer: &mut BinaryWriter,
    ) -> IoResult<()> {
        write_json_item(permission, writer)
    }

    fn deserialize_contract_permission(reader: &mut MemoryReader) -> IoResult<ContractPermission> {
        read_json_item(reader, MAX_SCRIPT_SIZE)
    }
}

impl Serializable for ContractManifest {
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        Self::deserialize_io(reader)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_io(writer)
    }

    fn size(&self) -> usize {
        self.size()
    }
}
