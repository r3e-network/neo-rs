use std::error::Error;
use std::fmt;

use elliptic_curve::sec1::ToEncodedPoint;
use elliptic_curve::p256::NistP256;
use elliptic_curve::PublicKey;
use crate::core::interop;
use crate::core::transaction;
use crate::crypto::keys;
use crate::smartcontract::callflag;
use crate::smartcontract::manifest;
use crate::util;
use crate::vm;
use crate::vm::stackitem;

pub fn check_hashed_witness(ic: &interop::Context, hash: util::Uint160) -> Result<bool, Box<dyn Error>> {
    let calling_sh = ic.vm.get_calling_script_hash();
    if !calling_sh.is_zero() && hash == calling_sh {
        return Ok(true);
    }
    check_scope(ic, hash)
}

struct ScopeContext<'a> {
    vm: &'a vm::VM,
    ic: &'a interop::Context,
}

impl<'a> ScopeContext<'a> {
    fn is_called_by_entry(&self) -> bool {
        self.vm.context().is_called_by_entry()
    }

    fn check_script_groups(&self, h: util::Uint160, k: &keys::PublicKey) -> Result<bool, Box<dyn Error>> {
        let groups = get_contract_groups(self.vm, self.ic, h)?;
        Ok(groups.contains(k))
    }

    fn calling_script_has_group(&self, k: &keys::PublicKey) -> Result<bool, Box<dyn Error>> {
        self.check_script_groups(self.vm.get_calling_script_hash(), k)
    }

    fn current_script_has_group(&self, k: &keys::PublicKey) -> Result<bool, Box<dyn Error>> {
        self.check_script_groups(self.vm.get_current_script_hash(), k)
    }
}

fn get_contract_groups(v: &vm::VM, ic: &interop::Context, h: util::Uint160) -> Result<manifest::Groups, Box<dyn Error>> {
    if !v.context().get_call_flags().has(callflag::READ_STATES) {
        return Err("missing ReadStates call flag".into());
    }
    let cs = ic.get_contract(h)?;
    if cs.is_none() {
        return Ok(manifest::Groups::new());
    }
    Ok(cs.unwrap().manifest.groups)
}

fn check_scope(ic: &interop::Context, hash: util::Uint160) -> Result<bool, Box<dyn Error>> {
    let signers = ic.signers();
    if signers.is_empty() {
        return Err("no valid signers".into());
    }
    for c in signers {
        if c.account == hash {
            if c.scopes == transaction::GLOBAL {
                return Ok(true);
            }
            if c.scopes.contains(transaction::CALLED_BY_ENTRY) {
                if ic.vm.context().is_called_by_entry() {
                    return Ok(true);
                }
            }
            if c.scopes.contains(transaction::CUSTOM_CONTRACTS) {
                let current_script_hash = ic.vm.get_current_script_hash();
                if c.allowed_contracts.contains(&current_script_hash) {
                    return Ok(true);
                }
            }
            if c.scopes.contains(transaction::CUSTOM_GROUPS) {
                let groups = get_contract_groups(ic.vm, ic, ic.vm.get_current_script_hash())?;
                if c.allowed_groups.iter().any(|g| groups.contains(g)) {
                    return Ok(true);
                }
            }
            if c.scopes.contains(transaction::RULES) {
                let ctx = ScopeContext { vm: ic.vm, ic };
                for r in &c.rules {
                    let res = r.condition.match_condition(&ctx)?;
                    if res {
                        return Ok(r.action == transaction::WITNESS_ALLOW);
                    }
                }
            }
            return Ok(false);
        }
    }
    Ok(false)
}

pub fn check_keyed_witness(ic: &interop::Context, key: &keys::PublicKey) -> Result<bool, Box<dyn Error>> {
    check_hashed_witness(ic, key.get_script_hash())
}

pub fn check_witness(ic: &interop::Context) -> Result<(), Box<dyn Error>> {
    let hash_or_key = ic.vm.estack().pop().bytes();
    let hash = util::Uint160::decode_bytes_be(&hash_or_key);
    let res = if let Ok(hash) = hash {
        check_hashed_witness(ic, hash)?
    } else {
        let key = PublicKey::from_sec1_bytes(&hash_or_key).map_err(|_| "parameter given is neither a key nor a hash")?;
        check_keyed_witness(ic, &keys::PublicKey::from(key))?
    };
    ic.vm.estack().push_item(stackitem::StackItem::Bool(res));
    Ok(())
}
