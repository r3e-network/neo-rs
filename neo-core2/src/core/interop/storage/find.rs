use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::thread;
use std::fmt;

use crate::core::interop::Context;
use crate::core::storage::{self, KeyValue};
use crate::vm::stackitem::{self, StackItem};

const FIND_DEFAULT: i64 = 0;
const FIND_KEYS_ONLY: i64 = 1 << 0;
const FIND_REMOVE_PREFIX: i64 = 1 << 1;
const FIND_VALUES_ONLY: i64 = 1 << 2;
const FIND_DESERIALIZE: i64 = 1 << 3;
const FIND_PICK0: i64 = 1 << 4;
const FIND_PICK1: i64 = 1 << 5;
const FIND_BACKWARDS: i64 = 1 << 7;

const FIND_ALL: i64 = FIND_DEFAULT | FIND_KEYS_ONLY | FIND_REMOVE_PREFIX | FIND_VALUES_ONLY |
    FIND_DESERIALIZE | FIND_PICK0 | FIND_PICK1 | FIND_BACKWARDS;

pub struct Iterator {
    seek_rx: Receiver<KeyValue>,
    curr: Option<KeyValue>,
    next: bool,
    opts: i64,
    prefix: Vec<u8>,
}

impl Iterator {
    pub fn new(seek_rx: Receiver<KeyValue>, prefix: Vec<u8>, opts: i64) -> Self {
        Self {
            seek_rx,
            curr: None,
            next: false,
            opts,
            prefix,
        }
    }

    pub fn next(&mut self) -> bool {
        match self.seek_rx.recv() {
            Ok(kv) => {
                self.curr = Some(kv);
                self.next = true;
            }
            Err(_) => {
                self.next = false;
            }
        }
        self.next
    }

    pub fn value(&self) -> StackItem {
        if !self.next {
            panic!("iterator index out of range");
        }
        let mut key = self.curr.as_ref().unwrap().key.clone();
        if self.opts & FIND_REMOVE_PREFIX == 0 {
            key = [self.prefix.clone(), key].concat();
        }
        if self.opts & FIND_KEYS_ONLY != 0 {
            return StackItem::ByteArray(key);
        }
        let mut value = StackItem::ByteArray(self.curr.as_ref().unwrap().value.clone());
        if self.opts & FIND_DESERIALIZE != 0 {
            value = stackitem::deserialize(&self.curr.as_ref().unwrap().value).unwrap();
        }
        if self.opts & FIND_PICK0 != 0 {
            value = value.value().as_array().unwrap()[0].clone();
        } else if self.opts & FIND_PICK1 != 0 {
            value = value.value().as_array().unwrap()[1].clone();
        }
        if self.opts & FIND_VALUES_ONLY != 0 {
            return value;
        }
        StackItem::Struct(vec![StackItem::ByteArray(key), value])
    }
}

pub fn find(ic: &mut Context) -> Result<(), Box<dyn std::error::Error>> {
    let stc_interface = ic.vm.estack().pop().value();
    let stc = stc_interface.downcast_ref::<Context>().ok_or_else(|| {
        format!("{} is not a storage::Context", stc_interface.type_id())
    })?;
    let prefix = ic.vm.estack().pop().bytes();
    let opts = ic.vm.estack().pop().bigint().to_i64().unwrap();
    if opts & !FIND_ALL != 0 {
        return Err(Box::new(fmt::Error::new(fmt::ErrorKind::InvalidInput, "unknown flag")));
    }
    if opts & FIND_KEYS_ONLY != 0 && opts & (FIND_DESERIALIZE | FIND_PICK0 | FIND_PICK1) != 0 {
        return Err(Box::new(fmt::Error::new(fmt::ErrorKind::InvalidInput, "KeysOnly conflicts with other options")));
    }
    if opts & FIND_VALUES_ONLY != 0 && opts & (FIND_KEYS_ONLY | FIND_REMOVE_PREFIX) != 0 {
        return Err(Box::new(fmt::Error::new(fmt::ErrorKind::InvalidInput, "KeysOnly conflicts with ValuesOnly")));
    }
    if opts & FIND_PICK0 != 0 && opts & FIND_PICK1 != 0 {
        return Err(Box::new(fmt::Error::new(fmt::ErrorKind::InvalidInput, "Pick0 conflicts with Pick1")));
    }
    if opts & FIND_DESERIALIZE == 0 && (opts & FIND_PICK0 != 0 || opts & FIND_PICK1 != 0) {
        return Err(Box::new(fmt::Error::new(fmt::ErrorKind::InvalidInput, "PickN is specified without Deserialize")));
    }
    let bkwrds = opts & FIND_BACKWARDS != 0;
    let (seek_tx, seek_rx) = std::sync::mpsc::channel();
    let ctx = Arc::new(Mutex::new(Some(seek_tx)));
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag_clone = cancel_flag.clone();
    let ctx_clone = ctx.clone();
    thread::spawn(move || {
        let seekres = ic.dao.seek_async(ctx_clone, stc.id, storage::SeekRange { prefix: prefix.clone(), backwards: bkwrds });
        for kv in seekres {
            if cancel_flag_clone.load(Ordering::Relaxed) {
                break;
            }
            ctx.lock().unwrap().as_ref().unwrap().send(kv).unwrap();
        }
    });
    let item = Iterator::new(seek_rx, prefix, opts);
    ic.vm.estack().push_item(StackItem::Interop(Box::new(item)));
    ic.register_cancel_func(move || {
        cancel_flag.store(true, Ordering::Relaxed);
        for _ in seek_rx {}
    });

    Ok(())
}
