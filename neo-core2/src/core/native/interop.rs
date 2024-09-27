use std::error::Error;
use std::fmt;

use crate::config::{Config, Hardfork};
use crate::core::interop::{Context, Contract};
use crate::smartcontract::{self, CallFlag};
use crate::vm::stackitem::Item;

/// Call calls the specified native contract method.
pub fn call(ic: &mut Context) -> Result<(), Box<dyn Error>> {
    let version = ic.vm.estack().pop().big_int().to_i64();
    if version != 0 {
        return Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, 
            format!("native contract of version {} is not active", version))));
    }

    let curr = ic.vm.get_current_script_hash();
    let c = ic.natives.iter().find(|ctr| ctr.metadata().hash == curr)
        .ok_or_else(|| fmt::Error::new(fmt::ErrorKind::Other, 
            format!("native contract {} (version {}) not found", curr.to_string_le(), version)))?;

    let generic_meta = c.metadata();
    if let Some(active_in) = c.active_in() {
        let height = ic.hardforks.get(&active_in.to_string())
            .ok_or_else(|| fmt::Error::new(fmt::ErrorKind::Other, 
                format!("native contract {} is active after hardfork {}", generic_meta.name, active_in)))?;
        if ic.block_height() < *height {
            return Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, 
                format!("native contract {} is active after hardfork {}", generic_meta.name, active_in))));
        }
    }

    let current = Config::hardforks().iter()
        .take_while(|&hf| ic.is_hardfork_enabled(*hf))
        .last()
        .copied()
        .unwrap_or_default();

    let meta = generic_meta.hf_specific_contract_md(&current);
    let m = meta.get_method_by_offset(ic.vm.context().ip())
        .ok_or_else(|| fmt::Error::new(fmt::ErrorKind::Other, "method not found"))?;

    let mut req_flags = m.required_flags;
    if !ic.is_hardfork_enabled(Hardfork::Aspidochelone) && meta.id == ManagementContractID &&
        (m.md.name == "deploy" || m.md.name == "update") {
        req_flags &= CallFlag::STATES | CallFlag::ALLOW_NOTIFY;
    }

    if !ic.vm.context().get_call_flags().has(req_flags) {
        return Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, 
            format!("missing call flags for native {} `{}` operation call: {:05b} vs {:05b}",
                version, m.md.name, ic.vm.context().get_call_flags(), req_flags))));
    }

    let invoke_fee = m.cpu_fee * ic.base_exec_fee() + m.storage_fee * ic.base_storage_fee();
    if !ic.vm.add_gas(invoke_fee) {
        return Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, "gas limit exceeded")));
    }

    let ctx = ic.vm.context();
    let args: Vec<Item> = (0..m.md.parameters.len())
        .map(|i| ic.vm.estack().peek(i).item().clone())
        .collect();

    let result = (m.func)(ic, &args);

    for _ in &m.md.parameters {
        ic.vm.estack().pop();
    }

    if m.md.return_type != smartcontract::VoidType {
        ctx.estack().push_item(result);
    }

    Ok(())
}

/// OnPersist calls OnPersist methods for all native contracts.
pub fn on_persist(ic: &mut Context) -> Result<(), Box<dyn Error>> {
    if ic.trigger != Trigger::OnPersist {
        return Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, "onPersist must be triggered by system")));
    }

    for c in &ic.natives {
        if let Some(active_in) = c.active_in() {
            if !ic.is_hardfork_enabled(*active_in) {
                continue;
            }
        }
        c.on_persist(ic)?;
    }

    Ok(())
}

/// PostPersist calls PostPersist methods for all native contracts.
pub fn post_persist(ic: &mut Context) -> Result<(), Box<dyn Error>> {
    if ic.trigger != Trigger::PostPersist {
        return Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, "postPersist must be triggered by system")));
    }

    for c in &ic.natives {
        if let Some(active_in) = c.active_in() {
            if !ic.is_hardfork_enabled(*active_in) {
                continue;
            }
        }
        c.post_persist(ic)?;
    }

    Ok(())
}
