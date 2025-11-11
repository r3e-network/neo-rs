use alloc::{collections::BTreeMap, vec, vec::Vec};

use neo_base::Bytes;
use neo_store::{ColumnId, MemoryStore};

use crate::{
    manifest::{
        ContractAbi, ContractFeatures, ContractManifest, ContractMethod, ContractParameter,
        ContractPermission, ParameterKind, WildcardContainer,
    },
    runtime::execution::ExecutionContext,
    runtime::Value,
};

pub(super) struct Column;

impl Column {
    pub const ID: ColumnId = ColumnId::new("counter");
}

pub(super) fn sample_manifest() -> ContractManifest {
    ContractManifest {
        name: "Counter".into(),
        groups: vec![],
        features: ContractFeatures::default(),
        supported_standards: vec![],
        abi: ContractAbi {
            methods: vec![
                ContractMethod {
                    name: "increment".into(),
                    parameters: vec![ContractParameter {
                        name: "key".into(),
                        kind: ParameterKind::ByteArray,
                    }],
                    return_type: ParameterKind::Integer,
                    offset: 0,
                    safe: false,
                },
                ContractMethod {
                    name: "reset".into(),
                    parameters: vec![ContractParameter {
                        name: "key".into(),
                        kind: ParameterKind::ByteArray,
                    }],
                    return_type: ParameterKind::ByteArray,
                    offset: 0,
                    safe: false,
                },
            ],
            events: Vec::new(),
        },
        permissions: vec![ContractPermission::allow_all()],
        trusts: WildcardContainer::wildcard(),
        extra: BTreeMap::new(),
    }
}

pub(super) fn sample_context<'a>() -> (MemoryStore, ExecutionContext<'a>) {
    let mut store = MemoryStore::new();
    store.create_column(Column::ID);
    let mut ctx = ExecutionContext::new(&mut store, 1_000, None);
    (store, ctx)
}
