use neo_base::Bytes;
use neo_store::ColumnId;

use crate::{error::VmError, runtime::RuntimeHost, value::VmValue};

use super::SyscallDispatcher;

pub(super) fn register_storage(dispatcher: &mut SyscallDispatcher) {
    dispatcher.register("System.Storage.GetContext", storage_get_context);
    dispatcher.register("System.Storage.Get", storage_get);
    dispatcher.register("System.Storage.Put", storage_put);
    dispatcher.register("System.Storage.Delete", storage_delete);
    dispatcher.register("System.Storage.Find", storage_find);
    dispatcher.register("System.Storage.Next", storage_next);
}

fn storage_get_context(host: &mut dyn RuntimeHost, _args: &[VmValue]) -> Result<VmValue, VmError> {
    Ok(VmValue::Bytes(host.storage_context_bytes()))
}

fn storage_get(host: &mut dyn RuntimeHost, args: &[VmValue]) -> Result<VmValue, VmError> {
    let column = parse_context(args.get(0))?;
    let key = args
        .get(1)
        .and_then(|value| value.as_bytes())
        .ok_or(VmError::InvalidType)?;
    match host
        .load_storage(column, key.as_slice())
        .map_err(|_| VmError::NativeFailure("storage get"))?
    {
        Some(bytes) => Ok(VmValue::Bytes(Bytes::from(bytes))),
        None => Ok(VmValue::Null),
    }
}

fn storage_put(host: &mut dyn RuntimeHost, args: &[VmValue]) -> Result<VmValue, VmError> {
    let column = parse_context(args.get(0))?;
    let key = args
        .get(1)
        .and_then(|value| value.as_bytes())
        .ok_or(VmError::InvalidType)?;
    let value = args
        .get(2)
        .and_then(|value| value.as_bytes())
        .ok_or(VmError::InvalidType)?;
    host.put_storage(column, key.as_slice(), value.as_slice())
        .map_err(|_| VmError::NativeFailure("storage put"))?;
    Ok(VmValue::Null)
}

fn storage_delete(host: &mut dyn RuntimeHost, args: &[VmValue]) -> Result<VmValue, VmError> {
    let column = parse_context(args.get(0))?;
    let key = args
        .get(1)
        .and_then(|value| value.as_bytes())
        .ok_or(VmError::InvalidType)?;
    host.delete_storage(column, key.as_slice())
        .map_err(|_| VmError::NativeFailure("storage delete"))?;
    Ok(VmValue::Null)
}

fn storage_find(host: &mut dyn RuntimeHost, args: &[VmValue]) -> Result<VmValue, VmError> {
    let column = parse_context(args.get(0))?;
    let prefix = args
        .get(1)
        .and_then(|value| value.as_bytes())
        .ok_or(VmError::InvalidType)?;
    let options = args
        .get(2)
        .and_then(|value| value.as_int())
        .ok_or(VmError::InvalidType)?;
    if !(0..=u8::MAX as i64).contains(&options) {
        return Err(VmError::InvalidType);
    }
    let handle = host
        .find_storage_iterator(column, prefix.as_slice(), options as u8)
        .map_err(|_| VmError::NativeFailure("storage find"))?;
    Ok(VmValue::Int(handle as i64))
}

fn storage_next(host: &mut dyn RuntimeHost, args: &[VmValue]) -> Result<VmValue, VmError> {
    let handle = args
        .get(0)
        .and_then(|value| value.as_int())
        .ok_or(VmError::InvalidType)?;
    if handle < 0 {
        return Err(VmError::InvalidType);
    }
    host.storage_iterator_next(handle as u32)
        .map_err(|_| VmError::NativeFailure("storage next"))?
        .map_or(Ok(VmValue::Null), Ok)
}

fn parse_context(value: Option<&VmValue>) -> Result<ColumnId, VmError> {
    let bytes = value
        .and_then(|val| val.as_bytes())
        .ok_or(VmError::InvalidType)?;
    if bytes.as_slice().starts_with(b"ctx:") {
        match &bytes.as_slice()[4..] {
            b"contract" => Ok(ColumnId::new("contract")),
            b"storage" => Ok(ColumnId::new("storage")),
            _ => Err(VmError::InvalidType),
        }
    } else {
        match bytes.as_slice() {
            b"contract" => Ok(ColumnId::new("contract")),
            b"storage" => Ok(ColumnId::new("storage")),
            _ => Err(VmError::InvalidType),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::{BTreeMap, VecDeque};

    use super::*;
    use crate::{runtime::Trigger, value::VmValue};
    use neo_base::hash::Hash160;

    #[test]
    fn storage_find_and_next_roundtrip() {
        let mut host = TestHost::with_entries(vec![
            (b"app:a".to_vec(), b"A".to_vec()),
            (b"app:b".to_vec(), b"B".to_vec()),
        ]);
        let args = vec![
            VmValue::Bytes(Bytes::from(b"contract".as_slice())),
            VmValue::Bytes(Bytes::from(b"app:".as_slice())),
            VmValue::Int(0),
        ];
        let handle = storage_find(&mut host, &args).unwrap();
        assert_eq!(handle, VmValue::Int(0));

        let first = storage_next(&mut host, &[VmValue::Int(0)]).unwrap();
        match first {
            VmValue::Array(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], VmValue::Bytes(Bytes::from(&b"app:a"[..])));
                assert_eq!(items[1], VmValue::Bytes(Bytes::from(&b"A"[..])));
            }
            other => panic!("expected array result, got {other:?}"),
        }
    }

    #[test]
    fn storage_next_returns_null_when_exhausted() {
        let mut host = TestHost::with_entries(vec![(b"app:a".to_vec(), b"A".to_vec())]);
        storage_find(
            &mut host,
            &[
                VmValue::Bytes(Bytes::from(b"contract".as_slice())),
                VmValue::Bytes(Bytes::from(b"app:".as_slice())),
                VmValue::Int(0),
            ],
        )
        .unwrap();
        let _ = storage_next(&mut host, &[VmValue::Int(0)]).unwrap();
        assert_eq!(
            storage_next(&mut host, &[VmValue::Int(0)]).unwrap(),
            VmValue::Null
        );
    }

    struct TestHost {
        data: BTreeMap<&'static str, Vec<(Vec<u8>, Vec<u8>)>>,
        iterators: Vec<Option<VecDeque<VmValue>>>,
    }

    impl TestHost {
        fn with_entries(entries: Vec<(Vec<u8>, Vec<u8>)>) -> Self {
            let mut data = BTreeMap::new();
            data.insert("contract", entries);
            Self {
                data,
                iterators: Vec::new(),
            }
        }
    }

    impl RuntimeHost for TestHost {
        fn log(&mut self, _message: String) {}

        fn notify(&mut self, _event: String, _payload: Vec<VmValue>) -> Result<(), VmError> {
            Ok(())
        }

        fn load_storage(
            &mut self,
            column: ColumnId,
            key: &[u8],
        ) -> Result<Option<Vec<u8>>, VmError> {
            Ok(self
                .data
                .get(column.name())
                .and_then(|entries| entries.iter().find(|(k, _)| k.as_slice() == key))
                .map(|(_, v)| v.clone()))
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
            column: ColumnId,
            prefix: &[u8],
            _options: u8,
        ) -> Result<u32, VmError> {
            let mut items = VecDeque::new();
            if let Some(entries) = self.data.get(column.name()) {
                for (key, value) in entries {
                    if key.starts_with(prefix) {
                        items.push_back(VmValue::Array(vec![
                            VmValue::Bytes(Bytes::from(key.clone())),
                            VmValue::Bytes(Bytes::from(value.clone())),
                        ]));
                    }
                }
            }
            let handle = self.iterators.len() as u32;
            self.iterators.push(Some(items));
            Ok(handle)
        }

        fn storage_iterator_next(&mut self, handle: u32) -> Result<Option<VmValue>, VmError> {
            if let Some(entry) = self.iterators.get_mut(handle as usize) {
                if let Some(queue) = entry {
                    if let Some(item) = queue.pop_front() {
                        if queue.is_empty() {
                            *entry = None;
                        }
                        return Ok(Some(item));
                    }
                    *entry = None;
                }
            }
            Ok(None)
        }

        fn timestamp(&self) -> i64 {
            0
        }

        fn invocation_counter(&self) -> u32 {
            0
        }

        fn storage_context_bytes(&self) -> Bytes {
            Bytes::from(b"contract".as_slice())
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
            0
        }
    }
}
