// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::vec::Vec;
use bytes::BytesMut;

#[cfg(feature = "std")]
use std::collections::HashMap;

#[cfg(not(feature = "std"))]
use hashbrown::HashMap;

use crate::{PublicKey, types::{Sign, Script, Varbytes}};


pub struct MultiSignContext<'a> {
    validators: &'a [PublicKey],
    arguments: Vec<Sign>,
    signs: HashMap<&'a PublicKey, usize>,
}


impl<'a> MultiSignContext<'a> {
    pub fn new(validators: &'a [PublicKey]) -> Self {
        Self {
            validators,
            arguments: Vec::with_capacity(validators.len()),
            signs: HashMap::with_capacity(validators.len()),
        }
    }

    pub fn signs_count(&self) -> usize { self.signs.len() }

    pub fn add_sign(&mut self, key: &'a PublicKey, sign: &Sign) -> bool {
        if self.signs.get(key).is_some() {
            return false;
        }

        if self.validators.iter().find(|pk| key.eq(pk)).is_none() {
            return false;
        }

        self.signs.insert(key, self.arguments.len());
        self.arguments.push(sign.clone());

        true
    }

    pub fn to_invocation_script(&self) -> Script {
        const SIGN_UNIT: usize = 34;
        let mut buf = BytesMut::with_capacity(self.signs_count() * SIGN_UNIT);
        for validator in self.validators.iter().rev() {
            if let Some(index) = self.signs.get(validator) {
                buf.put_varbytes(&self.arguments[*index])
            }
        }

        buf.into()
    }
}