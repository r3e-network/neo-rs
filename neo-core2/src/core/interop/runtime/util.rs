use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::convert::TryInto;
use std::cmp::min;

use bigdecimal::BigDecimal;
use byteorder::{ByteOrder, LittleEndian};
use murmur3::murmur3_x64_128;
use neo_core::config::Config;
use neo_core::interop::Context;
use neo_core::state::NotificationEvent;
use neo_core::util::Uint160;
use neo_core::vm::{self, stackitem::{StackItem, StackItemType}};
use neo_core::encoding::address::NEO3_PREFIX;

pub fn gas_left(ic: &mut Context) -> Result<(), Box<dyn Error>> {
    let gas_limit = ic.vm.gas_limit;
    let gas_left = if gas_limit == -1 {
        BigDecimal::from(gas_limit)
    } else {
        BigDecimal::from(gas_limit - ic.vm.gas_consumed())
    };
    ic.vm.estack().push_item(StackItem::BigInteger(gas_left));
    Ok(())
}

pub fn get_notifications(ic: &mut Context) -> Result<(), Box<dyn Error>> {
    let item = ic.vm.estack().pop().item();
    let mut notifications = ic.notifications.clone();
    if item.item_type() != StackItemType::Null {
        let b = item.try_bytes()?;
        let u = Uint160::from_bytes_be(&b)?;
        notifications = ic.notifications.iter()
            .filter(|n| n.script_hash == u)
            .cloned()
            .collect();
    }
    if notifications.len() > vm::MAX_STACK_SIZE {
        return Err("too many notifications".into());
    }
    let arr = StackItem::Array(notifications.iter().map(|n| {
        StackItem::Array(vec![
            StackItem::ByteArray(n.script_hash.to_bytes_be()),
            StackItem::from(n.name.clone()),
            n.item.clone(),
        ])
    }).collect());
    ic.vm.estack().push_item(arr);
    Ok(())
}

pub fn get_invocation_counter(ic: &mut Context) -> Result<(), Box<dyn Error>> {
    let current_script_hash = ic.vm.get_current_script_hash();
    let count = ic.invocations.entry(current_script_hash).or_insert(1);
    ic.vm.estack().push_item(StackItem::BigInteger(BigDecimal::from(*count)));
    Ok(())
}

pub fn get_address_version(ic: &mut Context) -> Result<(), Box<dyn Error>> {
    ic.vm.estack().push_item(StackItem::BigInteger(BigDecimal::from(NEO3_PREFIX)));
    Ok(())
}

pub fn get_network(ic: &mut Context) -> Result<(), Box<dyn Error>> {
    let m = ic.chain.get_config().magic;
    ic.vm.estack().push_item(StackItem::BigInteger(BigDecimal::from(m)));
    Ok(())
}

pub fn get_random(ic: &mut Context) -> Result<(), Box<dyn Error>> {
    let price: i64;
    let mut seed = ic.network;
    let is_hf = ic.is_hardfork_enabled(Config::HFAspidochelone);
    if is_hf {
        price = 1 << 13;
        seed += ic.get_random_counter.load(Ordering::SeqCst);
        ic.get_random_counter.fetch_add(1, Ordering::SeqCst);
    } else {
        price = 1 << 4;
    }
    let res = murmur128(&ic.nonce_data, seed)?;
    if !is_hf {
        ic.nonce_data.copy_from_slice(&res);
    }
    if !ic.vm.add_gas(ic.base_exec_fee() * price) {
        return Err("gas limit exceeded".into());
    }
    let mut res_le = res.to_vec();
    res_le.reverse();
    ic.vm.estack().push_item(StackItem::BigInteger(BigDecimal::from_bytes_le(&res_le)));
    Ok(())
}

fn murmur128(data: &[u8], seed: u32) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut result = vec![0; 16];
    murmur3_x64_128(data, seed as u64, &mut result)?;
    Ok(result)
}
