use std::error::Error;
use std::fmt;
use std::str::FromStr;

use crate::config;
use crate::core::dao;
use crate::core::interop;
use crate::core::native::nativenames;
use crate::core::state;
use crate::smartcontract;
use crate::smartcontract::callflag;
use crate::smartcontract::manifest;
use crate::util;
use crate::vm;
use crate::vm::stackitem;

trait PolicyChecker {
    fn is_blocked(&self, dao: &dao::Simple, hash: &util::Uint160) -> bool;
}

#[derive(Debug)]
struct CallError(String);

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for CallError {}

pub fn load_token(ic: &mut interop::Context, id: i32) -> Result<(), Box<dyn Error>> {
    let ctx = ic.vm.context();
    if !ctx.get_call_flags().contains(callflag::CallFlag::READ_STATES | callflag::CallFlag::ALLOW_CALL) {
        return Err(Box::new(CallError("invalid call flags".to_string())));
    }
    let tok = &ctx.get_nef().tokens[id as usize];
    if tok.param_count as usize > ctx.estack().len() {
        return Err(Box::new(CallError("stack is too small".to_string())));
    }
    let mut args = Vec::with_capacity(tok.param_count as usize);
    for _ in 0..tok.param_count {
        args.push(ic.vm.estack().pop().item());
    }
    let cs = ic.get_contract(&tok.hash)?;
    call_internal(ic, cs, &tok.method, tok.call_flag, tok.has_return, args, false)
}

pub fn call(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    let h = ic.vm.estack().pop().bytes();
    let method = ic.vm.estack().pop().string();
    let fs = callflag::CallFlag::from_bits(ic.vm.estack().pop().bigint().to_i64().unwrap() as i32)
        .ok_or_else(|| CallError("call flags out of range".to_string()))?;
    let args = ic.vm.estack().pop().array();
    let u = util::Uint160::from_bytes_be(&h).map_err(|_| CallError("invalid contract hash".to_string()))?;
    let cs = ic.get_contract(&u)?;
    if method.starts_with('_') {
        return Err(Box::new(CallError("invalid method name (starts with '_')".to_string())));
    }
    let md = cs.manifest.abi.get_method(&method, args.len())
        .ok_or_else(|| CallError(format!("method not found: {}/{}", method, args.len())))?;
    let has_return = md.return_type != smartcontract::ContractParameterType::Void;
    call_internal(ic, cs, &method, fs, has_return, args, true)
}

fn call_internal(ic: &mut interop::Context, cs: &state::Contract, name: &str, f: callflag::CallFlag,
                 has_return: bool, args: Vec<stackitem::Item>, is_dynamic: bool) -> Result<(), Box<dyn Error>> {
    let md = cs.manifest.abi.get_method(name, args.len())
        .ok_or_else(|| CallError(format!("method '{}' not found", name)))?;
    let mut f = f;
    if md.safe {
        f &= !(callflag::CallFlag::WRITE_STATES | callflag::CallFlag::ALLOW_NOTIFY);
    } else if let Some(ctx) = ic.vm.context() {
        if ctx.is_deployed() {
            let mfst = if ic.is_hardfork_enabled(config::HFDomovoi) {
                ctx.get_manifest()
            } else {
                ic.get_contract(&ic.vm.get_current_script_hash()).ok().map(|curr| &curr.manifest)
            };
            if let Some(mfst) = mfst {
                if !mfst.can_call(&cs.hash, &cs.manifest, name) {
                    return Err(Box::new(CallError("disallowed method call".to_string())));
                }
            }
        }
    }
    call_ex_from_native(ic, &ic.vm.get_current_script_hash(), cs, name, args, f, has_return, is_dynamic, false)
}

fn call_ex_from_native(ic: &mut interop::Context, caller: &util::Uint160, cs: &state::Contract,
                       name: &str, args: Vec<stackitem::Item>, f: callflag::CallFlag, has_return: bool, is_dynamic: bool, call_from_native: bool) -> Result<(), Box<dyn Error>> {
    for nc in &ic.natives {
        if nc.metadata().name == nativenames::POLICY {
            let pch = nc.as_any().downcast_ref::<dyn PolicyChecker>().unwrap();
            if pch.is_blocked(&ic.dao, &cs.hash) {
                return Err(Box::new(CallError(format!("contract {} is blocked", cs.hash.to_string_le()))));
            }
            break;
        }
    }
    let md = cs.manifest.abi.get_method(name, args.len())
        .ok_or_else(|| CallError(format!("method '{}' not found", name)))?;
    if args.len() != md.parameters.len() {
        return Err(Box::new(CallError(format!("invalid argument count: {} (expected {})", args.len(), md.parameters.len()))));
    }

    let method_off = md.offset;
    let init_off = cs.manifest.abi.get_method(manifest::METHOD_INIT, 0).map_or(-1, |md| md.offset);
    ic.invocations.entry(cs.hash.clone()).and_modify(|e| *e += 1).or_insert(1);
    let f = ic.vm.context().get_call_flags() & f;

    let wrapped = ic.vm.contract_has_try_block() &&
        f.intersects(callflag::CallFlag::ALL - callflag::CallFlag::READ_ONLY);
    let base_ntf_count = ic.notifications.len();
    let base_dao = ic.dao.clone();
    if wrapped {
        ic.dao = ic.dao.get_private();
    }
    let on_unload = move |v: &mut vm::VM, ctx: &mut vm::Context, commit: bool| -> Result<(), Box<dyn Error>> {
        if wrapped {
            if commit {
                ic.dao.persist().map_err(|e| CallError(format!("failed to persist changes: {}", e)))?;
            } else {
                ic.notifications.truncate(base_ntf_count);
            }
            ic.dao = base_dao.clone();
        }
        if call_from_native && !commit {
            return Err(Box::new(CallError("unhandled exception".to_string())));
        }
        if is_dynamic {
            return vm::dynamic_on_unload(v, ctx, commit);
        }
        Ok(())
    };
    ic.vm.load_nef_method(&cs.nef, &cs.manifest, caller, &cs.hash, f, has_return, method_off, init_off, on_unload)?;

    for arg in args.into_iter().rev() {
        ic.vm.estack().push_item(arg);
    }
    Ok(())
}

pub const ERR_NATIVE_CALL: &str = "failed native call";

pub fn call_from_native(ic: &mut interop::Context, caller: &util::Uint160, cs: &state::Contract, method: &str, args: Vec<stackitem::Item>, has_return: bool) -> Result<(), Box<dyn Error>> {
    let start_size = ic.vm.istack().len();
    call_ex_from_native(ic, caller, cs, method, args, callflag::CallFlag::ALL, has_return, false, true)?;

    while !ic.vm.has_stopped() && ic.vm.istack().len() > start_size {
        ic.vm.step().map_err(|e| CallError(format!("{}: {}", ERR_NATIVE_CALL, e)))?;
    }
    if ic.vm.has_failed() {
        return Err(Box::new(CallError(ERR_NATIVE_CALL.to_string())));
    }
    Ok(())
}

pub fn get_call_flags(ic: &mut interop::Context) -> Result<(), Box<dyn Error>> {
    ic.vm.estack().push_item(stackitem::Item::BigInteger(ic.vm.context().get_call_flags().bits().into()));
    Ok(())
}
