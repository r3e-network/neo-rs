use alloc::vec::Vec;

use neo_base::hash::Hash160;

use crate::{error::VmError, runtime::RuntimeHost, value::VmValue};

use super::SyscallDispatcher;

pub(super) fn register_contract(dispatcher: &mut SyscallDispatcher) {
    dispatcher.register("System.Contract.GetCallFlags", get_call_flags);
    dispatcher.register("System.Contract.Call", contract_call);
}

fn get_call_flags(host: &mut dyn RuntimeHost, _args: &[VmValue]) -> Result<VmValue, VmError> {
    Ok(VmValue::Int(host.call_flags() as i64))
}

fn contract_call(host: &mut dyn RuntimeHost, args: &[VmValue]) -> Result<VmValue, VmError> {
    let hash_bytes = args
        .get(0)
        .and_then(|value| value.as_bytes())
        .ok_or(VmError::InvalidType)?;
    let hash = Hash160::from_slice(hash_bytes.as_slice()).map_err(|_| VmError::InvalidType)?;
    let method = match args.get(1) {
        Some(VmValue::String(s)) => s.clone(),
        Some(VmValue::Bytes(bytes)) => {
            String::from_utf8(bytes.clone().into_vec()).map_err(|_| VmError::InvalidType)?
        }
        _ => return Err(VmError::InvalidType),
    };
    let flags = args
        .get(2)
        .and_then(|value| value.as_int())
        .ok_or(VmError::InvalidType)?;
    if flags < 0 || flags > u8::MAX as i64 {
        return Err(VmError::InvalidType);
    }
    let arg_array = match args.get(3) {
        Some(VmValue::Array(values)) => values.clone(),
        Some(_) => return Err(VmError::InvalidType),
        None => Vec::new(),
    };
    host.call_contract(&hash, &method, flags as u8, arg_array)
        .map_err(|_| VmError::NativeFailure("contract call failed"))
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
        last_call: Option<(Hash160, String, u8, Vec<VmValue>)>,
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

        fn call_contract(
            &mut self,
            hash: &Hash160,
            method: &str,
            call_flags: u8,
            args: Vec<VmValue>,
        ) -> Result<VmValue, VmError> {
            self.last_call = Some((*hash, method.to_string(), call_flags, args));
            Ok(VmValue::Int(42))
        }
    }

    #[test]
    fn returns_call_flags_bits() {
        let mut host = MockHost {
            flags: 0b1010,
            last_call: None,
        };
        let mut dispatcher = SyscallDispatcher::new(&mut host);
        let result = dispatcher
            .invoke("System.Contract.GetCallFlags", &[])
            .expect("call flags syscall");
        assert_eq!(result, VmValue::Int(0b1010));
    }

    #[test]
    fn forwards_contract_call_arguments() {
        let mut host = MockHost {
            flags: 0,
            last_call: None,
        };
        let mut dispatcher = SyscallDispatcher::new(&mut host);
        let hash = Hash160::from_slice(&[0x22; 20]).expect("hash160");
        let args = vec![
            VmValue::Bytes(Bytes::from(hash.to_vec())),
            VmValue::String("foo".into()),
            VmValue::Int(4),
            VmValue::Array(vec![VmValue::Int(7)]),
        ];
        let value = dispatcher
            .invoke("System.Contract.Call", &args)
            .expect("contract call");
        assert_eq!(value, VmValue::Int(42));
        let (called_hash, method, flags, call_args) = host.last_call.expect("call data recorded");
        assert_eq!(called_hash, hash);
        assert_eq!(method, "foo");
        assert_eq!(flags, 4);
        assert_eq!(call_args, vec![VmValue::Int(7)]);
    }
}
