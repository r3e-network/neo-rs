use std::error::Error;
use std::fmt;
use std::io::{self, Read, Write};

use crate::core::transaction::{self, Transaction};
use crate::crypto::hash;
use crate::crypto::keys;
use crate::util;
use crate::vm::opcode;

// P2PNotaryRequest contains main and fallback transactions for the Notary service.
#[derive(Clone)]
pub struct P2PNotaryRequest {
    pub main_transaction: Transaction,
    pub fallback_transaction: Transaction,
    pub witness: transaction::Witness,
    hash: util::Uint256,
}

impl P2PNotaryRequest {
    // NewP2PNotaryRequestFromBytes decodes a P2PNotaryRequest from the given bytes.
    pub fn from_bytes(b: &[u8]) -> Result<Self, Box<dyn Error>> {
        let mut req = P2PNotaryRequest {
            main_transaction: Transaction::default(),
            fallback_transaction: Transaction::default(),
            witness: transaction::Witness::default(),
            hash: util::Uint256::default(),
        };
        let mut br = io::Cursor::new(b);
        req.decode_binary(&mut br)?;
        if br.position() != b.len() as u64 {
            return Err("additional data after the payload".into());
        }
        Ok(req)
    }

    // Bytes returns serialized P2PNotaryRequest payload.
    pub fn bytes(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut buf = Vec::new();
        self.encode_binary(&mut buf)?;
        Ok(buf)
    }

    // Hash returns payload's hash.
    pub fn hash(&mut self) -> util::Uint256 {
        if self.hash == util::Uint256::default() {
            if self.create_hash().is_err() {
                panic!("failed to compute hash!");
            }
        }
        self.hash
    }

    // createHash creates hash of the payload.
    fn create_hash(&mut self) -> Result<(), Box<dyn Error>> {
        let mut buf = Vec::new();
        self.encode_hashable_fields(&mut buf)?;
        self.hash = hash::sha256(&buf);
        Ok(())
    }

    // DecodeBinaryUnsigned reads payload from the w excluding signature.
    fn decode_hashable_fields<R: Read>(&mut self, br: &mut R) -> Result<(), Box<dyn Error>> {
        self.main_transaction.decode_binary(br)?;
        self.fallback_transaction.decode_binary(br)?;
        self.is_valid()?;
        self.create_hash()?;
        Ok(())
    }

    // DecodeBinary implements the io::Serializable interface.
    pub fn decode_binary<R: Read>(&mut self, br: &mut R) -> Result<(), Box<dyn Error>> {
        self.decode_hashable_fields(br)?;
        self.witness.decode_binary(br)?;
        Ok(())
    }

    // encodeHashableFields writes payload to the w excluding signature.
    fn encode_hashable_fields<W: Write>(&self, bw: &mut W) -> Result<(), Box<dyn Error>> {
        self.main_transaction.encode_binary(bw)?;
        self.fallback_transaction.encode_binary(bw)?;
        Ok(())
    }

    // EncodeBinary implements the Serializable interface.
    pub fn encode_binary<W: Write>(&self, bw: &mut W) -> Result<(), Box<dyn Error>> {
        self.encode_hashable_fields(bw)?;
        self.witness.encode_binary(bw)?;
        Ok(())
    }

    pub fn is_valid(&self) -> Result<(), Box<dyn Error>> {
        let n_keys_main = self.main_transaction.get_attributes(transaction::NotaryAssistedT);
        if n_keys_main.is_empty() {
            return Err("main transaction should have NotaryAssisted attribute".into());
        }
        if n_keys_main[0].value.as_ref().unwrap().n_keys == 0 {
            return Err("main transaction should have NKeys > 0".into());
        }
        if self.fallback_transaction.signers.len() != 2 {
            return Err("fallback transaction should have two signers".into());
        }
        if self.fallback_transaction.scripts[0].invocation_script.len() != 66
            || !self.fallback_transaction.scripts[0].verification_script.is_empty()
            || !self.fallback_transaction.scripts[0]
                .invocation_script
                .starts_with(&[opcode::PUSHDATA1 as u8, keys::SIGNATURE_LEN as u8])
        {
            return Err("fallback transaction has invalid dummy Notary witness".into());
        }
        if !self
            .fallback_transaction
            .has_attribute(transaction::NotValidBeforeT)
        {
            return Err("fallback transactions should have NotValidBefore attribute".into());
        }
        let conflicts = self
            .fallback_transaction
            .get_attributes(transaction::ConflictsT);
        if conflicts.len() != 1 {
            return Err("fallback transaction should have one Conflicts attribute".into());
        }
        if conflicts[0].value.as_ref().unwrap().hash != self.main_transaction.hash() {
            return Err("fallback transaction does not conflict with the main transaction".into());
        }
        let n_keys_fallback = self
            .fallback_transaction
            .get_attributes(transaction::NotaryAssistedT);
        if n_keys_fallback.is_empty() {
            return Err("fallback transaction should have NotaryAssisted attribute".into());
        }
        if n_keys_fallback[0].value.as_ref().unwrap().n_keys != 0 {
            return Err("fallback transaction should have NKeys = 0".into());
        }
        if self.main_transaction.valid_until_block != self.fallback_transaction.valid_until_block {
            return Err("both main and fallback transactions should have the same ValidUntil value".into());
        }
        Ok(())
    }

    // Copy creates a deep copy of P2PNotaryRequest. It creates deep copy of the MainTransaction,
    // FallbackTransaction and Witness, including all slice fields. Cached values like
    // 'hashed' and 'size' of the transactions are reset to ensure the copy can be modified
    // independently of the original.
    pub fn copy(&self) -> Self {
        P2PNotaryRequest {
            main_transaction: self.main_transaction.clone(),
            fallback_transaction: self.fallback_transaction.clone(),
            witness: self.witness.clone(),
            hash: self.hash,
        }
    }
}
