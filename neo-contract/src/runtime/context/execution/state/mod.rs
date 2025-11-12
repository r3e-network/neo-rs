mod builder;
mod view;

use alloc::{string::String, vec::Vec};

use neo_base::{hash::Hash160, Bytes};
use neo_core::tx::Signer;
use neo_crypto::ecc256::PublicKey;
use neo_store::Store;
use neo_vm::Trigger;

use super::super::storage::StorageContext;
use crate::{
    nef::CallFlags,
    runtime::{gas::GasMeter, storage::StorageIterator, value::Value},
};

pub struct ExecutionContext<'a> {
    pub(super) store: &'a mut dyn Store,
    pub(super) gas: GasMeter,
    pub(super) legacy_signer: Option<Hash160>,
    pub(super) signers: Vec<Signer>,
    pub(super) log: Vec<String>,
    pub(super) notifications: Vec<(String, Vec<Value>)>,
    pub(super) timestamp: i64,
    pub(super) invocation_counter: u32,
    pub(super) storage_context: StorageContext,
    pub(super) script: Bytes,
    pub(super) current_script_hash: Option<Hash160>,
    pub(super) entry_script_hash: Option<Hash160>,
    pub(super) calling_script_hash: Option<Hash160>,
    pub(super) current_contract_groups: Vec<PublicKey>,
    pub(super) calling_contract_groups: Vec<PublicKey>,
    pub(super) current_call_flags: CallFlags,
    pub(super) trigger: Trigger,
    pub(super) platform: String,
    pub(super) storage_iterators: Vec<Option<StorageIterator>>,
}

pub use builder::ExecutionContextBuilder;
pub use view::ExecutionContextView;
