use std::error::Error;
use std::sync::Arc;
use std::sync::Mutex;
use crate::network::payload;
use crate::util;
use crate::crypto::hash;
use crate::core::transaction;
use crate::network::extpool::Pool;
use crate::network::extpool::test_chain::TestChain;
use crate::network::extpool::test_chain::new_test_chain;
use crate::network::extpool::test_chain::ERR_VERIFICATION;
use crate::network::extpool::test_chain::ERR_DISALLOWED_SENDER;
use crate::network::extpool::test_chain::ERR_INVALID_HEIGHT;

#[test]
fn test_add_get() {
    let bc = new_test_chain();
    bc.lock().unwrap().height = 10;

    let p = Pool::new(bc.clone(), 100);
    t.run("invalid witness", |t| {
        let ep = payload::Extensible { valid_block_end: 100, sender: util::Uint160([0x42]) };
        p.test_add(t, false, Some(ERR_VERIFICATION), ep);
    });
    t.run("disallowed sender", |t| {
        let ep = payload::Extensible { valid_block_end: 100, sender: util::Uint160([0x41]) };
        p.test_add(t, false, Some(ERR_DISALLOWED_SENDER), ep);
    });
    t.run("bad height", |t| {
        let ep = payload::Extensible { valid_block_end: 9, sender: util::Uint160([0x00]) };
        p.test_add(t, false, Some(ERR_INVALID_HEIGHT), ep);

        let ep = payload::Extensible { valid_block_end: 10, sender: util::Uint160([0x00]) };
        p.test_add(t, false, None, ep);
    });
    t.run("good", |t| {
        let ep = payload::Extensible { valid_block_end: 100, sender: util::Uint160([0x00]) };
        p.test_add(t, true, None, ep);
        assert_eq!(ep, p.get(ep.hash()));

        p.test_add(t, false, None, ep);
    });
}

#[test]
fn test_capacity_limit() {
    let bc = new_test_chain();
    bc.lock().unwrap().height = 10;

    t.run("invalid capacity", |t| {
        assert_panic!(Pool::new(bc.clone(), 0));
    });

    let p = Pool::new(bc.clone(), 3);

    let first = payload::Extensible { valid_block_end: 11, sender: util::Uint160([0x00]) };
    p.test_add(t, true, None, first);

    for height in &[12, 13] {
        let ep = payload::Extensible { valid_block_end: *height, sender: util::Uint160([0x00]) };
        p.test_add(t, true, None, ep);
    }

    assert!(p.get(first.hash()).is_some());

    let (ok, err) = p.add(payload::Extensible { valid_block_end: 14, sender: util::Uint160([0x00]) });
    assert!(ok);
    assert!(err.is_none());

    assert!(p.get(first.hash()).is_none());
}

// This test checks that sender count is updated
// when oldest payload is removed during `Add`.
#[test]
fn test_decrease_sender_on_evict() {
    let bc = new_test_chain();
    bc.lock().unwrap().height = 10;

    let p = Pool::new(bc.clone(), 2);
    let senders = vec![util::Uint160([1]), util::Uint160([2]), util::Uint160([3])];
    for i in 11..17 {
        let ep = payload::Extensible { sender: senders[(i % 3) as usize], valid_block_end: i };
        p.test_add(t, true, None, ep);
    }
}

#[test]
fn test_remove_stale() {
    let bc = new_test_chain();
    bc.lock().unwrap().height = 10;

    let p = Pool::new(bc.clone(), 100);
    let eps = vec![
        payload::Extensible { valid_block_end: 11, sender: util::Uint160([0x00]) }, // small height
        payload::Extensible { valid_block_end: 12, sender: util::Uint160([0x00]) }, // good
        payload::Extensible { valid_block_end: 12, sender: util::Uint160([0x11]) }, // invalid sender
        payload::Extensible { valid_block_end: 12, sender: util::Uint160([0x12]) }, // invalid witness
    ];
    for ep in &eps {
        p.test_add(t, true, None, ep.clone());
    }
    bc.lock().unwrap().verify_witness = Box::new(|u| u[0] != 0x12);
    bc.lock().unwrap().is_allowed = Box::new(|u| u[0] != 0x11);
    p.remove_stale(11);
    assert!(p.get(eps[0].hash()).is_none());
    assert_eq!(eps[1], p.get(eps[1].hash()).unwrap());
    assert!(p.get(eps[2].hash()).is_none());
    assert!(p.get(eps[3].hash()).is_none());
}

impl Pool {
    fn test_add(&self, t: &mut Test, expected_ok: bool, expected_err: Option<&'static str>, ep: payload::Extensible) {
        let (ok, err) = self.add(ep);
        if let Some(expected_err) = expected_err {
            assert_eq!(err.unwrap().description(), expected_err);
        } else {
            assert!(err.is_none());
        }
        assert_eq!(expected_ok, ok);
    }
}

pub struct TestChain {
    pub ledger: Ledger,
    pub height: u32,
    pub verify_witness: Box<dyn Fn(util::Uint160) -> bool + Send + Sync>,
    pub is_allowed: Box<dyn Fn(util::Uint160) -> bool + Send + Sync>,
}

pub const ERR_VERIFICATION: &str = "verification failed";

pub fn new_test_chain() -> Arc<Mutex<TestChain>> {
    Arc::new(Mutex::new(TestChain {
        ledger: Ledger::default(),
        height: 0,
        verify_witness: Box::new(|u| u[0] != 0x42),
        is_allowed: Box::new(|u| u[0] != 0x42 && u[0] != 0x41),
    }))
}

impl TestChain {
    pub fn verify_witness(&self, u: util::Uint160, _hashable: &dyn hash::Hashable, _witness: &transaction::Witness, _gas: i64) -> Result<i64, &'static str> {
        if !(self.verify_witness)(u) {
            return Err(ERR_VERIFICATION);
        }
        Ok(0)
    }

    pub fn is_extensible_allowed(&self, u: util::Uint160) -> bool {
        (self.is_allowed)(u)
    }

    pub fn block_height(&self) -> u32 {
        self.height
    }
}
