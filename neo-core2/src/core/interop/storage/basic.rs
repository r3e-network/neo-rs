use std::error::Error;
use std::fmt;

use crate::config::limits;
use crate::core::interop;
use crate::vm::stackitem;

#[derive(Debug)]
pub struct GasLimitExceededError;

impl fmt::Display for GasLimitExceededError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "gas limit exceeded")
    }
}

impl Error for GasLimitExceededError {}

#[derive(Debug)]
pub struct FindInvalidOptionsError;

impl fmt::Display for FindInvalidOptionsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid Find options")
    }
}

impl Error for FindInvalidOptionsError {}

#[derive(Debug)]
pub struct Context {
    id: i32,
    read_only: bool,
}

pub fn delete(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    let stc_interface = ic.vm.estack().pop().value();
    let stc = stc_interface.downcast_ref::<Context>().ok_or_else(|| {
        format!("{} is not a storage::Context", stc_interface.type_id())
    })?;
    if stc.read_only {
        return Err("storage::Context is read only".into());
    }
    let key = ic.vm.estack().pop().bytes();
    ic.dao.delete_storage_item(stc.id, &key);
    Ok(())
}

pub fn get(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    let stc_interface = ic.vm.estack().pop().value();
    let stc = stc_interface.downcast_ref::<Context>().ok_or_else(|| {
        format!("{} is not a storage::Context", stc_interface.type_id())
    })?;
    let key = ic.vm.estack().pop().bytes();
    let si = ic.dao.get_storage_item(stc.id, &key);
    if let Some(si) = si {
        ic.vm.estack().push_item(stackitem::StackItem::ByteArray(si.to_vec()));
    } else {
        ic.vm.estack().push_item(stackitem::StackItem::Null);
    }
    Ok(())
}

pub fn get_context(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    get_context_internal(ic, false)
}

pub fn get_read_only_context(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    get_context_internal(ic, true)
}

fn get_context_internal(ic: &mut interop::Context, is_read_only: bool) -> Result<(), Box<dyn Error>> {
    let contract = ic.get_contract(ic.vm.get_current_script_hash())?;
    let sc = Context {
        id: contract.id,
        read_only: is_read_only,
    };
    ic.vm.estack().push_item(stackitem::StackItem::Interop(Box::new(sc)));
    Ok(())
}

fn put_with_context(ic: &mut interop::Context, stc: &Context, key: &[u8], value: &[u8]) -> Result<(), Box<dyn Error>> {
    if key.len() > limits::MAX_STORAGE_KEY_LEN {
        return Err("key is too big".into());
    }
    if value.len() > limits::MAX_STORAGE_VALUE_LEN {
        return Err("value is too big".into());
    }
    if stc.read_only {
        return Err("storage::Context is read only".into());
    }
    let si = ic.dao.get_storage_item(stc.id, key);
    let mut size_inc = value.len();
    if si.is_none() {
        size_inc = key.len() + value.len();
    } else if !value.is_empty() {
        if value.len() <= si.unwrap().len() {
            size_inc = (value.len() - 1) / 4 + 1;
        } else if !si.unwrap().is_empty() {
            size_inc = (si.unwrap().len() - 1) / 4 + 1 + value.len() - si.unwrap().len();
        }
    }
    if !ic.vm.add_gas((size_inc as i64) * ic.base_storage_fee()) {
        return Err(Box::new(GasLimitExceededError));
    }
    ic.dao.put_storage_item(stc.id, key, value);
    Ok(())
}

pub fn put(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    let stc_interface = ic.vm.estack().pop().value();
    let stc = stc_interface.downcast_ref::<Context>().ok_or_else(|| {
        format!("{} is not a storage::Context", stc_interface.type_id())
    })?;
    let key = ic.vm.estack().pop().bytes();
    let value = ic.vm.estack().pop().bytes();
    put_with_context(ic, stc, &key, &value)
}

pub fn context_as_read_only(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    let stc_interface = ic.vm.estack().pop().value();
    let stc = stc_interface.downcast_ref::<Context>().ok_or_else(|| {
        format!("{} is not a storage::Context", stc_interface.type_id())
    })?;
    if !stc.read_only {
        let stx = Context {
            id: stc.id,
            read_only: true,
        };
        ic.vm.estack().push_item(stackitem::StackItem::Interop(Box::new(stx)));
    } else {
        ic.vm.estack().push_item(stackitem::StackItem::Interop(Box::new(stc.clone())));
    }
    Ok(())
}
