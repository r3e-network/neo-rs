// Copyright (C) 2015-2025 The Neo Project.
//
// RestErrorCodes mirrors Neo.Plugins.RestServer.Exceptions.RestErrorCodes.
// It centralises the numeric codes emitted by RestServer exceptions.

pub struct RestErrorCodes;

impl RestErrorCodes {
    /// Generic catch-all error code used by the REST server (matches value 1000 in C#).
    pub const GENERIC_EXCEPTION: i32 = 1000;
    /// Parameter format/validation error code (matches value 1001 in C#).
    pub const PARAMETER_FORMAT_EXCEPTION: i32 = 1001;
}
