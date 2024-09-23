use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;
use bigdecimal::BigDecimal;
use crate::core::transaction::Transaction;
use crate::neorpc::result::Iterator;
use crate::rpcclient::unwrap;
use crate::util::{self, Uint160, Uint256};
use crate::rpcclient::Invoker;
use crate::rpcclient::Actor;

// DivisibleReader is a reader interface for divisible NEP-11 contract.
pub struct DivisibleReader {
    base_reader: BaseReader,
}

// DivisibleWriter is a state-changing interface for divisible NEP-11 contract.
// It's mostly useful not directly, but as a reusable layer for higher-level
// structures.
pub struct DivisibleWriter {
    base_writer: BaseWriter,
}

// Divisible is a full reader interface for divisible NEP-11 contract.
pub struct Divisible {
    divisible_reader: DivisibleReader,
    divisible_writer: DivisibleWriter,
}

// OwnerIterator is used for iterating over OwnerOf (for divisible NFTs) results.
pub struct OwnerIterator {
    client: Arc<dyn Invoker>,
    session: Uuid,
    iterator: Iterator,
}

// NewDivisibleReader creates an instance of DivisibleReader for a contract
// with the given hash using the given invoker.
impl DivisibleReader {
    pub fn new(invoker: Arc<dyn Invoker>, hash: Uint160) -> Self {
        DivisibleReader {
            base_reader: BaseReader::new(invoker, hash),
        }
    }
}

// NewDivisible creates an instance of Divisible for a contract
// with the given hash using the given actor.
impl Divisible {
    pub fn new(actor: Arc<dyn Actor>, hash: Uint160) -> Self {
        Divisible {
            divisible_reader: DivisibleReader::new(actor.clone(), hash),
            divisible_writer: DivisibleWriter {
                base_writer: BaseWriter::new(hash, actor),
            },
        }
    }
}

// OwnerOf returns an iterator that allows to walk through all owners of
// the given token. It depends on the server to provide proper session-based
// iterator, but can also work with expanded one.
impl DivisibleReader {
    pub fn owner_of(&self, token: &[u8]) -> Result<OwnerIterator, Box<dyn std::error::Error>> {
        let (sess, iter) = unwrap::session_iterator(self.base_reader.invoker.call(self.base_reader.hash, "ownerOf", token))?;
        Ok(OwnerIterator {
            client: self.base_reader.invoker.clone(),
            session: sess,
            iterator: iter,
        })
    }

    // OwnerOfExpanded uses the same NEP-11 method as OwnerOf, but can be useful if
    // the server used doesn't support sessions and doesn't expand iterators. It
    // creates a script that will get num of result items from the iterator right in
    // the VM and return them to you. It's only limited by VM stack and GAS available
    // for RPC invocations.
    pub fn owner_of_expanded(&self, token: &[u8], num: i32) -> Result<Vec<Uint160>, Box<dyn std::error::Error>> {
        unwrap::array_of_uint160(self.base_reader.invoker.call_and_expand_iterator(self.base_reader.hash, "ownerOf", num, token))
    }

    // BalanceOfD is a BalanceOf for divisible NFTs, it returns the amount of token
    // owned by a particular account.
    pub fn balance_of_d(&self, owner: Uint160, token: &[u8]) -> Result<BigDecimal, Box<dyn std::error::Error>> {
        unwrap::big_int(self.base_reader.invoker.call(self.base_reader.hash, "balanceOf", owner, token))
    }
}

// TransferD is a divisible version of (*Base).Transfer, allowing to transfer a
// part of NFT. It creates and sends a transaction that performs a `transfer`
// method call using the given parameters and checks for this call result,
// failing the transaction if it's not true. The returned values are transaction
// hash, its ValidUntilBlock value and an error if any.
impl DivisibleWriter {
    pub fn transfer_d(&self, from: Uint160, to: Uint160, amount: &BigDecimal, id: &[u8], data: &dyn std::any::Any) -> Result<(Uint256, u32), Box<dyn std::error::Error>> {
        let script = self.base_writer.transfer_script(from, to, amount, id, data)?;
        self.base_writer.actor.send_run(script)
    }

    // TransferDTransaction is a divisible version of (*Base).TransferTransaction,
    // allowing to transfer a part of NFT. It creates a transaction that performs a
    // `transfer` method call using the given parameters and checks for this call
    // result, failing the transaction if it's not true. This transaction is signed,
    // but not sent to the network, instead it's returned to the caller.
    pub fn transfer_d_transaction(&self, from: Uint160, to: Uint160, amount: &BigDecimal, id: &[u8], data: &dyn std::any::Any) -> Result<Transaction, Box<dyn std::error::Error>> {
        let script = self.base_writer.transfer_script(from, to, amount, id, data)?;
        self.base_writer.actor.make_run(script)
    }

    // TransferDUnsigned is a divisible version of (*Base).TransferUnsigned,
    // allowing to transfer a part of NFT. It creates a transaction that performs a
    // `transfer` method call using the given parameters and checks for this call
    // result, failing the transaction if it's not true. This transaction is not
    // signed and just returned to the caller.
    pub fn transfer_d_unsigned(&self, from: Uint160, to: Uint160, amount: &BigDecimal, id: &[u8], data: &dyn std::any::Any) -> Result<Transaction, Box<dyn std::error::Error>> {
        let script = self.base_writer.transfer_script(from, to, amount, id, data)?;
        self.base_writer.actor.make_unsigned_run(script, None)
    }
}

// Next returns the next set of elements from the iterator (up to num of them).
// It can return less than num elements in case iterator doesn't have that many
// or zero elements if the iterator has no more elements or the session is
// expired.
impl OwnerIterator {
    pub fn next(&self, num: i32) -> Result<Vec<Uint160>, Box<dyn std::error::Error>> {
        let items = self.client.traverse_iterator(self.session, &self.iterator, num)?;
        let mut res = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            let b = item.try_bytes()?;
            let u = Uint160::from_str(&hex::encode(b))?;
            res.push(u);
        }
        Ok(res)
    }

    // Terminate closes the iterator session used by OwnerIterator (if it's
    // session-based).
    pub fn terminate(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.iterator.id.is_none() {
            return Ok(());
        }
        self.client.terminate_session(self.session)
    }
}
