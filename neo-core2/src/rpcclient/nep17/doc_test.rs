use std::context::Context;
use std::str::FromStr;
use std::sync::Arc;
use bigdecimal::BigDecimal;
use neo_core2::rpcclient::{self, actor, invoker, nep17};
use neo_core2::util::address;
use neo_core2::util::Uint160;
use neo_core2::wallet::Wallet;

fn example_token_reader() {
    // No error checking done at all, intentionally.
    let c = rpcclient::RpcClient::new(Context::new(), "url", rpcclient::Options::default()).unwrap();

    // Safe methods are reachable with just an invoker, no need for an account there.
    let inv = invoker::Invoker::new(Arc::new(c), None);

    // NEP-17 contract hash.
    let nep17_hash = Uint160::from([9, 8, 7]);

    // And a reader interface.
    let n17 = nep17::Nep17Reader::new(inv, nep17_hash);

    // Get the metadata. Even though these methods are implemented in neptoken package,
    // they're available for NEP-17 wrappers.
    let symbol = n17.symbol().unwrap();
    let supply = n17.total_supply().unwrap();
    println!("Symbol: {}, Supply: {}", symbol, supply);

    // Account hash we're interested in.
    let acc_hash = address::string_to_uint160("NdypBhqkz2CMMnwxBgvoC9X2XjKF5axgKo").unwrap();

    // Get account balance.
    let balance = n17.balance_of(acc_hash).unwrap();
    println!("Balance: {}", balance);
}

fn example_token() {
    // No error checking done at all, intentionally.
    let mut w = Wallet::new_from_file("somewhere").unwrap();
    defer!(w.close());

    let c = rpcclient::RpcClient::new(Context::new(), "url", rpcclient::Options::default()).unwrap();

    // Create a simple CalledByEntry-scoped actor (assuming there is an account
    // inside the wallet).
    let a = actor::SimpleActor::new(Arc::new(c), w.accounts[0].clone()).unwrap();

    // NEP-17 contract hash.
    let nep17_hash = Uint160::from([9, 8, 7]);

    // Create a complete NEP-17 contract representation.
    let n17 = nep17::Nep17::new(a.clone(), nep17_hash);

    let tgt_acc = address::string_to_uint160("NdypBhqkz2CMMnwxBgvoC9X2XjKF5axgKo").unwrap();

    // Send a transaction that transfers one token to another account.
    let (txid, vub) = n17.transfer(a.sender(), tgt_acc, BigDecimal::from(1), None).unwrap();
    println!("Transaction ID: {}, VUB: {}", txid, vub);
}
