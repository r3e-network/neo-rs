use std::error::Error;
use std::fmt;
use std::sync::Arc;
use bigdecimal::BigDecimal;
use log::info;
use neo_core::config;
use neo_core::core::interop;
use neo_core::core::transaction;
use neo_core::smartcontract::callflag;
use neo_core::vm;
use neo_core::vm::stackitem::{self, StackItem};
use neo_core::vm::VM;

trait Itemable {
    fn to_stack_item(&self) -> StackItem;
}

const MAX_EVENT_NAME_LEN: usize = 32;
const MAX_NOTIFICATION_SIZE: usize = 1024;
const SYSTEM_RUNTIME_LOG_MESSAGE: &str = "runtime log";

pub fn get_executing_script_hash(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    ic.vm.push_context_script_hash(0)
}

pub fn get_calling_script_hash(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    let h = ic.vm.get_calling_script_hash();
    ic.vm.estack().push_item(StackItem::ByteArray(h.to_bytes_be()));
    Ok(())
}

pub fn get_entry_script_hash(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    ic.vm.push_context_script_hash(ic.vm.istack().len() - 1)
}

pub fn get_script_container(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    if let Some(c) = ic.container.as_ref().and_then(|c| c.as_any().downcast_ref::<dyn Itemable>()) {
        ic.vm.estack().push_item(c.to_stack_item());
        Ok(())
    } else {
        Err("unknown script container".into())
    }
}

pub fn platform(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    ic.vm.estack().push_item(StackItem::ByteArray(b"NEO".to_vec()));
    Ok(())
}

pub fn get_trigger(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    ic.vm.estack().push_item(StackItem::BigInteger(BigDecimal::from(ic.trigger as i64)));
    Ok(())
}

pub fn notify(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    let name = ic.vm.estack().pop().to_string();
    let elem = ic.vm.estack().pop();
    let args = elem.array();
    if name.len() > MAX_EVENT_NAME_LEN {
        return Err(format!("event name must be less than {}", MAX_EVENT_NAME_LEN).into());
    }
    let curr = ic.vm.context().get_manifest();
    if curr.is_none() {
        return Err("notifications are not allowed in dynamic scripts".into());
    }
    let curr = curr.unwrap();
    let ev = curr.abi.get_event(&name);
    let mut check_err = None;
    let cur_hash = ic.vm.get_current_script_hash();
    if ev.is_none() {
        check_err = Some(format!("notification {} does not exist", name));
    } else {
        if let Err(err) = ev.unwrap().check_compliance(&args) {
            check_err = Some(format!("notification {} is invalid: {}", name, err));
        }
    }
    if let Some(err) = check_err {
        if ic.is_hardfork_enabled(config::HFBasilisk) {
            return Err(err.into());
        }
        info!("bad notification", "contract" => cur_hash.to_string_le(), "event" => name, "error" => err);
    }

    let bytes = ic.dao.get_item_ctx().serialize(&elem.item(), false)?;
    if bytes.len() > MAX_NOTIFICATION_SIZE {
        return Err(format!("notification size shouldn't exceed {}", MAX_NOTIFICATION_SIZE).into());
    }
    ic.add_notification(cur_hash, name, stackitem::deep_copy(&StackItem::Array(args), true).as_array().unwrap().clone());
    Ok(())
}

pub fn load_script(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    let script = ic.vm.estack().pop().bytes();
    let fs = callflag::CallFlag::from_bits_truncate(ic.vm.estack().pop().bigint().to_i64().unwrap() as i32);
    if fs.bits() & !callflag::ALL.bits() != 0 {
        return Err("call flags out of range".into());
    }
    let args = ic.vm.estack().pop().array();
    vm::is_script_correct(&script, None)?;
    let fs = ic.vm.context().get_call_flags() & callflag::READ_ONLY & fs;
    ic.vm.load_dynamic_script(&script, fs);

    for i in (0..args.len()).rev() {
        ic.vm.estack().push_item(args[i].clone());
    }
    Ok(())
}

pub fn log(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    let state = ic.vm.estack().pop().to_string();
    if state.len() > MAX_NOTIFICATION_SIZE {
        return Err(format!("message length shouldn't exceed {}", MAX_NOTIFICATION_SIZE).into());
    }
    let tx_hash = ic.tx.as_ref().map(|tx| tx.hash().to_string_le()).unwrap_or_default();
    info!(SYSTEM_RUNTIME_LOG_MESSAGE, "tx" => tx_hash, "script" => ic.vm.get_current_script_hash().to_string_le(), "msg" => state);
    Ok(())
}

pub fn get_time(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    ic.vm.estack().push_item(StackItem::BigInteger(BigDecimal::from(ic.block.timestamp as u64)));
    Ok(())
}

pub fn burn_gas(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    let gas = ic.vm.estack().pop().bigint();
    if !gas.is_i64() {
        return Err("invalid GAS value".into());
    }

    let g = gas.to_i64().unwrap();
    if g <= 0 {
        return Err("GAS must be positive".into());
    }

    if !ic.vm.add_gas(g) {
        return Err("GAS limit exceeded".into());
    }
    Ok(())
}

pub fn current_signers(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    if let Some(tx) = ic.container.as_ref().and_then(|c| c.as_any().downcast_ref::<transaction::Transaction>()) {
        ic.vm.estack().push_item(transaction::signers_to_stack_item(&tx.signers));
    } else {
        ic.vm.estack().push_item(StackItem::Null);
    }
    Ok(())
}
