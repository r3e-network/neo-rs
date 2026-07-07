//! Runtime inventory lookup for utility RPC methods.
//!
//! `listplugins` and `listservices` gather local service/plugin facts here and
//! delegate response-shape construction to the sibling response module.

mod plugins;
mod services;
