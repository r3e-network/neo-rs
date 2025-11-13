use crate::{error::VmError, runtime::RuntimeHost, value::VmValue};

use super::SyscallDispatcher;

pub(super) fn register_contract(dispatcher: &mut SyscallDispatcher) {
    dispatcher.register("System.Contract.GetCallFlags", get_call_flags);
}

fn get_call_flags(host: &mut dyn RuntimeHost, _args: &[VmValue]) -> Result<VmValue, VmError> {
    Ok(VmValue::Int(host.call_flags() as i64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        runtime::{RuntimeHost, Trigger},
        value::VmValue,
    };
    use neo_base::{hash::Hash160, Bytes};
    use neo_store::ColumnId;

    struct MockHost {
        flags: u8,
    }

    impl RuntimeHost for MockHost {
        fn log(&mut self, _message: String) {}

        fn notify(&mut self, _event: String, _payload: Vec<VmValue>) -> Result<(), VmError> {
            Ok(())
        }

        fn load_storage(
            &mut self,
            _column: ColumnId,
            _key: &[u8],
        ) -> Result<Option<Vec<u8>>, VmError> {
            Ok(None)
        }

        fn put_storage(
            &mut self,
            _column: ColumnId,
            _key: &[u8],
            _value: &[u8],
        ) -> Result<(), VmError> {
            Ok(())
        }

        fn delete_storage(&mut self, _column: ColumnId, _key: &[u8]) -> Result<(), VmError> {
            Ok(())
        }

        fn find_storage_iterator(
            &mut self,
            _column: ColumnId,
            _prefix: &[u8],
            _options: u8,
        ) -> Result<u32, VmError> {
            Ok(0)
        }

        fn storage_iterator_next(&mut self, _handle: u32) -> Result<Option<VmValue>, VmError> {
            Ok(None)
        }

        fn timestamp(&self) -> i64 {
            0
        }

        fn invocation_counter(&self) -> u32 {
            0
        }

        fn storage_context_bytes(&self) -> Bytes {
            Bytes::default()
        }

        fn script(&self) -> Bytes {
            Bytes::default()
        }

        fn script_hash(&self) -> Option<Hash160> {
            None
        }

        fn calling_script_hash(&self) -> Option<Hash160> {
            None
        }

        fn entry_script_hash(&self) -> Option<Hash160> {
            None
        }

        fn platform(&self) -> &str {
            "NEO"
        }

        fn trigger(&self) -> Trigger {
            Trigger::Application
        }

        fn check_witness(&self, _target: &Hash160) -> bool {
            false
        }

        fn call_flags(&self) -> u8 {
            self.flags
        }
    }

    #[test]
    fn returns_call_flags_bits() {
        let mut host = MockHost { flags: 0b1010 };
        let mut dispatcher = SyscallDispatcher::new(&mut host);
        let result = dispatcher
            .invoke("System.Contract.GetCallFlags", &[])
            .expect("call flags syscall");
        assert_eq!(result, VmValue::Int(0b1010));
    }
}
