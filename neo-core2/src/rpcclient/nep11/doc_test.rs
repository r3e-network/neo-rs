use std::context::Context;
use std::bigint::BigInt;

use neo_core2::encoding::address;
use neo_core2::rpcclient::{self, actor, invoker, nep11};
use neo_core2::util;
use neo_core2::wallet;

fn example_non_divisible_reader() {
    // No error checking done at all, intentionally.
    let c = rpcclient::RpcClient::new(Context::background(), "url", rpcclient::Options::default()).unwrap();

    // Safe methods are reachable with just an invoker, no need for an account there.
    let inv = invoker::Invoker::new(c, None);

    // NEP-11 contract hash.
    let nep11_hash = util::Uint160::from([9, 8, 7]);

    // Most of the time contracts are non-divisible, create a reader for nep11_hash.
    let n11 = nep11::NonDivisibleReader::new(inv, nep11_hash);

    // Get the metadata. Even though these methods are implemented in neptoken package,
    // they're available for NEP-11 wrappers.
    let symbol = n11.symbol().unwrap();
    let supply = n11.total_supply().unwrap();
    let _ = symbol;
    let _ = supply;

    // Account hash we're interested in.
    let acc_hash = address::string_to_uint160("NdypBhqkz2CMMnwxBgvoC9X2XjKF5axgKo").unwrap();

    // Get account balance.
    let balance = n11.balance_of(acc_hash).unwrap();
    if balance.sign() > 0 {
        // There are some tokens there, let's look at them.
        let mut tok_iter = n11.tokens_of(acc_hash).unwrap();

        while let Ok((toks, err)) = tok_iter.next(10) {
            if err.is_none() && !toks.is_empty() {
                for tok in toks {
                    // We know the owner of the token, but let's check internal contract consistency.
                    let owner = n11.owner_of(tok).unwrap();
                    if !owner.equals(&acc_hash) {
                        panic!("NEP-11 contract is broken!");
                    }
                }
            } else {
                break;
            }
        }
    }
}

fn example_divisible_reader() {
    // No error checking done at all, intentionally.
    let c = rpcclient::RpcClient::new(Context::background(), "url", rpcclient::Options::default()).unwrap();

    // Safe methods are reachable with just an invoker, no need for an account there.
    let inv = invoker::Invoker::new(c, None);

    // NEP-11 contract hash.
    let nep11_hash = util::Uint160::from([9, 8, 7]);

    // Divisible contract are more rare, but we can handle them too.
    let n11 = nep11::DivisibleReader::new(inv, nep11_hash);

    // Get the metadata. Even though these methods are implemented in neptoken package,
    // they're available for NEP-11 wrappers.
    let symbol = n11.symbol().unwrap();
    let supply = n11.total_supply().unwrap();
    let _ = symbol;
    let _ = supply;

    // Account hash we're interested in.
    let acc_hash = address::string_to_uint160("NdypBhqkz2CMMnwxBgvoC9X2XjKF5axgKo").unwrap();

    // Get account balance.
    let balance = n11.balance_of(acc_hash).unwrap();
    if balance.sign() > 0 && balance.cmp(&BigInt::from(10)) < 0 {
        // We know we have a low number of tokens, so we can use a simple API to get them.
        let toks = n11.tokens_of_expanded(acc_hash, 10).unwrap();

        // We can build a list of all owners of account's tokens.
        let mut owners = Vec::new();
        for tok in toks {
            let mut own_iter = n11.owner_of(tok).unwrap();
            while let Ok((ows, err)) = own_iter.next(10) {
                if err.is_none() && !ows.is_empty() {
                    // Notice that it includes acc_hash too.
                    owners.extend(ows);
                } else {
                    break;
                }
            }
        }
        // The list can be sorted/deduplicated if needed.
        let _ = owners;
    }
}

fn example_non_divisible() {
    // No error checking done at all, intentionally.
    let mut w = wallet::Wallet::new_from_file("somewhere").unwrap();
    defer!(w.close());

    let c = rpcclient::RpcClient::new(Context::background(), "url", rpcclient::Options::default()).unwrap();

    // Create a simple CalledByEntry-scoped actor (assuming there is an account
    // inside the wallet).
    let a = actor::SimpleActor::new(c, &w.accounts[0]).unwrap();

    // NEP-11 contract hash.
    let nep11_hash = util::Uint160::from([9, 8, 7]);

    // Create a complete non-divisible contract representation.
    let n11 = nep11::NonDivisible::new(a, nep11_hash);

    let tgt_acc = address::string_to_uint160("NdypBhqkz2CMMnwxBgvoC9X2XjKF5axgKo").unwrap();

    // Let's transfer all of account's tokens to some other account.
    let mut tok_iter = n11.tokens_of(a.sender()).unwrap();
    while let Ok((toks, err)) = tok_iter.next(10) {
        if err.is_none() && !toks.is_empty() {
            for tok in toks {
                // This creates a transaction for every token, but you can
                // create a script that will move multiple tokens in one
                // transaction with Builder from smartcontract package.
                let (txid, vub) = n11.transfer(tgt_acc, tok, None).unwrap();
                let _ = txid;
                let _ = vub;
            }
        } else {
            break;
        }
    }
}

fn example_divisible() {
    // No error checking done at all, intentionally.
    let mut w = wallet::Wallet::new_from_file("somewhere").unwrap();
    defer!(w.close());

    let c = rpcclient::RpcClient::new(Context::background(), "url", rpcclient::Options::default()).unwrap();

    // Create a simple CalledByEntry-scoped actor (assuming there is an account
    // inside the wallet).
    let a = actor::SimpleActor::new(c, &w.accounts[0]).unwrap();

    // NEP-11 contract hash.
    let nep11_hash = util::Uint160::from([9, 8, 7]);

    // Create a complete divisible contract representation.
    let n11 = nep11::Divisible::new(a, nep11_hash);

    let tgt_acc = address::string_to_uint160("NdypBhqkz2CMMnwxBgvoC9X2XjKF5axgKo").unwrap();

    // Let's transfer all of account's tokens to some other account.
    let mut tok_iter = n11.tokens_of(a.sender()).unwrap();
    while let Ok((toks, err)) = tok_iter.next(10) {
        if err.is_none() && !toks.is_empty() {
            for tok in toks {
                // It's a divisible token, so balance data is required in general case.
                let balance = n11.balance_of_d(a.sender(), tok).unwrap();

                // This creates a transaction for every token, but you can
                // create a script that will move multiple tokens in one
                // transaction with Builder from smartcontract package.
                let (txid, vub) = n11.transfer_d(a.sender(), tgt_acc, balance, tok, None).unwrap();
                let _ = txid;
                let _ = vub;
            }
        } else {
            break;
        }
    }
}
