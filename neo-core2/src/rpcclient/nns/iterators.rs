use std::error::Error;
use std::fmt;
use uuid::Uuid;
use crate::neorpc::result::Iterator;
use crate::vm::stackitem::Item;
use crate::rpcclient::Invoker;

// RecordIterator is used for iterating over GetAllRecords results.
pub struct RecordIterator {
    client: Box<dyn Invoker>,
    session: Uuid,
    iterator: Iterator,
}

// RootIterator is used for iterating over Roots results.
pub struct RootIterator {
    client: Box<dyn Invoker>,
    session: Uuid,
    iterator: Iterator,
}

fn items_to_records(arr: Vec<Item>) -> Result<Vec<RecordState>, Box<dyn Error>> {
    let mut res = Vec::with_capacity(arr.len());
    for (i, item) in arr.into_iter().enumerate() {
        let mut record = RecordState::default();
        if let Err(err) = record.from_stack_item(item) {
            return Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, format!("item #{}: {}", i, err))));
        }
        res.push(record);
    }
    Ok(res)
}

fn items_to_roots(arr: Vec<Item>) -> Result<Vec<String>, Box<dyn Error>> {
    let mut res = Vec::with_capacity(arr.len());
    for item in arr {
        let rs = item.value().as_array().ok_or_else(|| fmt::Error::new(fmt::ErrorKind::Other, "wrong number of elements"))?;
        let myval = rs[0].try_bytes().map_err(|_| fmt::Error::new(fmt::ErrorKind::Other, "failed to convert to bytes"))?;
        res.push(String::from_utf8(myval).map_err(|_| fmt::Error::new(fmt::ErrorKind::Other, "failed to convert to string"))?);
    }
    Ok(res)
}

impl RecordIterator {
    // Next returns the next set of elements from the iterator (up to num of them).
    // It can return less than num elements in case iterator doesn't have that many
    // or zero elements if the iterator has no more elements or the session is
    // expired.
    pub fn next(&mut self, num: usize) -> Result<Vec<RecordState>, Box<dyn Error>> {
        let items = self.client.traverse_iterator(self.session, &self.iterator, num)?;
        items_to_records(items)
    }

    // Terminate closes the iterator session used by RecordIterator (if it's
    // session-based).
    pub fn terminate(&self) -> Result<(), Box<dyn Error>> {
        if self.iterator.id.is_none() {
            return Ok(());
        }
        self.client.terminate_session(self.session)
    }
}

impl RootIterator {
    // Next returns the next set of elements from the iterator (up to num of them).
    // It can return less than num elements in case iterator doesn't have that many
    // or zero elements if the iterator has no more elements or the session is
    // expired.
    pub fn next(&mut self, num: usize) -> Result<Vec<String>, Box<dyn Error>> {
        let items = self.client.traverse_iterator(self.session, &self.iterator, num)?;
        items_to_roots(items)
    }

    // Terminate closes the iterator session used by RootIterator (if it's
    // session-based).
    pub fn terminate(&self) -> Result<(), Box<dyn Error>> {
        if self.iterator.id.is_none() {
            return Ok(());
        }
        self.client.terminate_session(self.session)
    }
}
