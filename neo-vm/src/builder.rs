// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use bytes::BytesMut;
use neo_base::errors;

#[derive(Debug, errors::Error)]
pub enum EmitError {
    //
}

pub struct ScriptBuilder {
    buf: BytesMut,
}

impl ScriptBuilder {
    pub fn new() -> Self { Self { buf: BytesMut::new() } }
}
