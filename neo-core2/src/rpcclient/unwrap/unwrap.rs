/*
Package unwrap provides a set of proxy methods to process invocation results.

Functions implemented there are intended to be used as wrappers for other
functions that return (*result.Invoke, error) pair (of which there are many).
These functions will check for error, check for VM state, check the number
of results, cast them to appropriate type (if everything is OK) and then
return a result or error. They're mostly useful for other higher-level
contract-specific packages.
*/

use std::error::Error;
use std::fmt;
use std::str;
use std::string::FromUtf8Error;
use std::sync::Arc;
use std::convert::TryFrom;

use elliptic_curve::sec1::ToEncodedPoint;
use k256::elliptic_curve::sec1::EncodedPoint;
use k256::PublicKey;
use uuid::Uuid;

use crate::crypto::keys;
use crate::neorpc::result;
use crate::util;
use crate::vm::stackitem;
use crate::vm::vmstate;

// Exception is a type used for VM fault messages (aka exceptions). If any of
// unwrapper functions encounters a FAULT VM state it creates an instance of
// this type as an error using exception string. It can be used with [errors.As]
// to get the exact message from VM and compare with known contract-specific
// errors.
#[derive(Debug)]
struct Exception(String);

impl fmt::Display for Exception {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for Exception {}

// ErrNoSessionID is returned from the SessionIterator when the server does not
// have sessions enabled and does not perform automatic iterator expansion. It
// means you have no way to get the data from returned iterators using this
// server, other than expanding it in the VM script.
const ERR_NO_SESSION_ID: &str = "server returned iterator ID, but no session ID";

// BigInt expects correct execution (HALT state) with a single stack item
// returned. A big.Int is extracted from this item and returned.
fn big_int(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<big::BigInt, Box<dyn Error>> {
    let itm = item(r, err)?;
    itm.try_integer()
}

// Bool expects correct execution (HALT state) with a single stack item
// returned. A bool is extracted from this item and returned.
fn bool(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<bool, Box<dyn Error>> {
    let itm = item(r, err)?;
    itm.try_bool()
}

// Int64 expects correct execution (HALT state) with a single stack item
// returned. An int64 is extracted from this item and returned.
fn int64(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<i64, Box<dyn Error>> {
    let itm = item(r, err)?;
    let i = itm.try_integer()?;
    if !i.is_i64() {
        return Err(Box::new(Exception("int64 overflow".to_string())));
    }
    Ok(i.to_i64().unwrap())
}

// LimitedInt64 is similar to Int64 except it allows to set minimum and maximum
// limits to be checked, so if it doesn't return an error the value is more than
// min and less than max.
fn limited_int64(r: &result::Invoke, err: Option<Box<dyn Error>>, min_i: i64, max_i: i64) -> Result<i64, Box<dyn Error>> {
    let i = int64(r, err)?;
    if i < min_i {
        return Err(Box::new(Exception("too small value".to_string())));
    }
    if i > max_i {
        return Err(Box::new(Exception("too big value".to_string())));
    }
    Ok(i)
}

// Bytes expects correct execution (HALT state) with a single stack item
// returned. A slice of bytes is extracted from this item and returned.
fn bytes(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<Vec<u8>, Box<dyn Error>> {
    let itm = item(r, err)?;
    itm.try_bytes()
}

// UTF8String expects correct execution (HALT state) with a single stack item
// returned. A string is extracted from this item and checked for UTF-8
// correctness, valid strings are then returned.
fn utf8_string(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<String, Box<dyn Error>> {
    let b = bytes(r, err)?;
    if !str::from_utf8(&b).is_ok() {
        return Err(Box::new(Exception("not a UTF-8 string".to_string())));
    }
    Ok(String::from_utf8(b)?)
}

// PrintableASCIIString expects correct execution (HALT state) with a single
// stack item returned. A string is extracted from this item and checked to
// only contain ASCII characters in printable range, valid strings are then
// returned.
fn printable_ascii_string(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<String, Box<dyn Error>> {
    let s = utf8_string(r, err)?;
    for c in s.chars() {
        if c < ' ' || c >= '\x7f' {
            return Err(Box::new(Exception("not a printable ASCII string".to_string())));
        }
    }
    Ok(s)
}

// Uint160 expects correct execution (HALT state) with a single stack item
// returned. An util.Uint160 is extracted from this item and returned.
fn uint160(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<util::Uint160, Box<dyn Error>> {
    let b = bytes(r, err)?;
    util::Uint160::decode_bytes_be(&b)
}

// Uint256 expects correct execution (HALT state) with a single stack item
// returned. An util.Uint256 is extracted from this item and returned.
fn uint256(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<util::Uint256, Box<dyn Error>> {
    let b = bytes(r, err)?;
    util::Uint256::decode_bytes_be(&b)
}

// PublicKey expects correct execution (HALT state) with a single stack item
// returned. A public key is extracted from this item and returned.
fn public_key(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<PublicKey, Box<dyn Error>> {
    let b = bytes(r, err)?;
    PublicKey::from_sec1_bytes(&b).map_err(|e| Box::new(e) as Box<dyn Error>)
}

// SessionIterator expects correct execution (HALT state) with a single stack
// item returned. If this item is an iterator it's returned to the caller along
// with the session ID. Notice that this function also returns successfully
// with zero session ID (but an appropriate Iterator holding all the data
// received) when RPC server performs (limited) iterator expansion which is the
// default behavior for NeoGo servers with SessionEnabled set to false.
fn session_iterator(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<(Uuid, result::Iterator), Box<dyn Error>> {
    let itm = item(r, err)?;
    let iter = item_to_session_iterator(itm)?;
    if r.session == Uuid::nil() && iter.id.is_some() {
        return Err(Box::new(Exception(ERR_NO_SESSION_ID.to_string())));
    }
    Ok((r.session, iter))
}

// ArrayAndSessionIterator expects correct execution (HALT state) with one or two stack
// items returned. If there is 1 item, it must be an array. If there is a second item,
// it must be an iterator. This is exactly the result of smartcontract.CreateCallAndPrefetchIteratorScript.
// Sessions must be enabled on the RPC server for this to function correctly.
fn array_and_session_iterator(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<(Vec<stackitem::Item>, Uuid, result::Iterator), Box<dyn Error>> {
    check_res_ok(r, err)?;
    if r.stack.is_empty() {
        return Err(Box::new(Exception("result stack is empty".to_string())));
    }
    if r.stack.len() != 1 && r.stack.len() != 2 {
        return Err(Box::new(Exception(format!("expected 1 or 2 result items, got {}", r.stack.len()))));
    }

    // Unwrap array.
    let itm = &r.stack[0];
    let arr = itm.value().as_array().ok_or_else(|| Exception("not an array".to_string()))?;

    // Check whether iterator exists and unwrap it.
    if r.stack.len() == 1 {
        return Ok((arr.clone(), Uuid::nil(), result::Iterator::default()));
    }

    let iter = item_to_session_iterator(&r.stack[1])?;
    if r.session == Uuid::nil() {
        return Err(Box::new(Exception(ERR_NO_SESSION_ID.to_string())));
    }
    Ok((arr.clone(), r.session, iter))
}

fn item_to_session_iterator(itm: &stackitem::Item) -> Result<result::Iterator, Box<dyn Error>> {
    if itm.item_type() != stackitem::ItemType::Interop {
        return Err(Box::new(Exception(format!("expected InteropInterface, got {:?}", itm.item_type()))));
    }
    itm.value().as_iterator().ok_or_else(|| Exception("the item is InteropInterface, but not an Iterator".to_string()))
}

// Array expects correct execution (HALT state) with a single array stack item
// returned. This item is returned to the caller. Notice that this function can
// be used for structures as well since they're also represented as slices of
// stack items (the number of them and their types are structure-specific).
fn array(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<Vec<stackitem::Item>, Box<dyn Error>> {
    let itm = item(r, err)?;
    itm.value().as_array().ok_or_else(|| Exception("not an array".to_string()))
}

// ArrayOfBools checks the result for correct state (HALT) and then extracts a
// slice of boolean values from the returned stack item.
fn array_of_bools(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<Vec<bool>, Box<dyn Error>> {
    let a = array(r, err)?;
    let mut res = Vec::with_capacity(a.len());
    for (i, itm) in a.iter().enumerate() {
        let b = itm.try_bool().map_err(|e| Exception(format!("element {} is not a boolean: {}", i, e)))?;
        res.push(b);
    }
    Ok(res)
}

// ArrayOfBigInts checks the result for correct state (HALT) and then extracts a
// slice of (big) integer values from the returned stack item.
fn array_of_bigints(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<Vec<big::BigInt>, Box<dyn Error>> {
    let a = array(r, err)?;
    let mut res = Vec::with_capacity(a.len());
    for (i, itm) in a.iter().enumerate() {
        let v = itm.try_integer().map_err(|e| Exception(format!("element {} is not an integer: {}", i, e)))?;
        res.push(v);
    }
    Ok(res)
}

// ArrayOfBytes checks the result for correct state (HALT) and then extracts a
// slice of byte slices from the returned stack item.
fn array_of_bytes(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<Vec<Vec<u8>>, Box<dyn Error>> {
    let a = array(r, err)?;
    let mut res = Vec::with_capacity(a.len());
    for (i, itm) in a.iter().enumerate() {
        let b = itm.try_bytes().map_err(|e| Exception(format!("element {} is not a byte string: {}", i, e)))?;
        res.push(b);
    }
    Ok(res)
}

// ArrayOfUTF8Strings checks the result for correct state (HALT) and then extracts a
// slice of UTF-8 strings from the returned stack item.
fn array_of_utf8_strings(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<Vec<String>, Box<dyn Error>> {
    let a = array(r, err)?;
    let mut res = Vec::with_capacity(a.len());
    for (i, itm) in a.iter().enumerate() {
        let b = itm.try_bytes().map_err(|e| Exception(format!("element {} is not a byte string: {}", i, e)))?;
        if !str::from_utf8(&b).is_ok() {
            return Err(Box::new(Exception(format!("element {} is not a UTF-8 string", i))));
        }
        res.push(String::from_utf8(b)?);
    }
    Ok(res)
}

// ArrayOfUint160 checks the result for correct state (HALT) and then extracts a
// slice of util.Uint160 from the returned stack item.
fn array_of_uint160(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<Vec<util::Uint160>, Box<dyn Error>> {
    let a = array(r, err)?;
    let mut res = Vec::with_capacity(a.len());
    for (i, itm) in a.iter().enumerate() {
        let b = itm.try_bytes().map_err(|e| Exception(format!("element {} is not a byte string: {}", i, e)))?;
        let u = util::Uint160::decode_bytes_be(&b).map_err(|e| Exception(format!("element {} is not a uint160: {}", i, e)))?;
        res.push(u);
    }
    Ok(res)
}

// ArrayOfUint256 checks the result for correct state (HALT) and then extracts a
// slice of util.Uint256 from the returned stack item.
fn array_of_uint256(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<Vec<util::Uint256>, Box<dyn Error>> {
    let a = array(r, err)?;
    let mut res = Vec::with_capacity(a.len());
    for (i, itm) in a.iter().enumerate() {
        let b = itm.try_bytes().map_err(|e| Exception(format!("element {} is not a byte string: {}", i, e)))?;
        let u = util::Uint256::decode_bytes_be(&b).map_err(|e| Exception(format!("element {} is not a uint256: {}", i, e)))?;
        res.push(u);
    }
    Ok(res)
}

// ArrayOfPublicKeys checks the result for correct state (HALT) and then
// extracts a slice of public keys from the returned stack item.
fn array_of_public_keys(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<Vec<PublicKey>, Box<dyn Error>> {
    let arr = array(r, err)?;
    let mut pks = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let val = item.try_bytes().map_err(|e| Exception(format!("invalid array element #{}: {:?}", i, item.item_type())))?;
        let pk = PublicKey::from_sec1_bytes(&val).map_err(|e| Exception(format!("array element #{} in not a key: {}", i, e)))?;
        pks.push(pk);
    }
    Ok(pks)
}

// Map expects correct execution (HALT state) with a single stack item
// returned. A stackitem.Map is extracted from this item and returned.
fn map(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<stackitem::Map, Box<dyn Error>> {
    let itm = item(r, err)?;
    if itm.item_type() != stackitem::ItemType::Map {
        return Err(Box::new(Exception(format!("{:?} is not a map", itm.item_type()))));
    }
    Ok(itm.as_map().unwrap().clone())
}

fn check_res_ok(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<(), Box<dyn Error>> {
    if let Some(e) = err {
        return Err(e);
    }
    if r.state != vmstate::VMState::Halt {
        return Err(Box::new(Exception(format!("invocation failed: {}", r.fault_exception))));
    }
    if !r.fault_exception.is_empty() {
        return Err(Box::new(Exception(format!("inconsistent result, HALTed with exception: {}", r.fault_exception))));
    }
    Ok(())
}

// Item returns a stack item from the result if execution was successful (HALT
// state) and if it's the only element on the result stack.
fn item(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<stackitem::Item, Box<dyn Error>> {
    check_res_ok(r, err)?;
    if r.stack.is_empty() {
        return Err(Box::new(Exception("result stack is empty".to_string())));
    }
    if r.stack.len() > 1 {
        return Err(Box::new(Exception(format!("too many ({}) result items", r.stack.len()))));
    }
    Ok(r.stack[0].clone())
}

// Nothing expects zero stack items and a successful invocation (HALT state).
fn nothing(r: &result::Invoke, err: Option<Box<dyn Error>>) -> Result<(), Box<dyn Error>> {
    check_res_ok(r, err)?;
    if !r.stack.is_empty() {
        return Err(Box::new(Exception("result stack is not empty".to_string())));
    }
    Ok(())
}
