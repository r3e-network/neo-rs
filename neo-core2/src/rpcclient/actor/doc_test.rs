use std::fs::File;
use std::io::Write;
use std::context::Context;
use serde_json::json;
use neo_core2::rpcclient::{RpcClient, Options};
use neo_core2::rpcclient::actor::{Actor, SignerAccount, DefaultCheckerModifier, NewDefaultOptions};
use neo_core2::rpcclient::neo::NeoContract;
use neo_core2::rpcclient::policy::PolicyContract;
use neo_core2::smartcontract::context::ParameterContext;
use neo_core2::transaction::{Transaction, Attribute, Signer, HighPriority, None};
use neo_core2::util::Uint160;
use neo_core2::vm::vmstate::VMState;
use neo_core2::wallet::Wallet;

fn example_actor() {
    // No error checking done at all, intentionally.
    let mut w = Wallet::new_wallet_from_file("somewhere").unwrap();
    defer!(w.close());

    let c = RpcClient::new(Context::background(), "url", Options::default()).unwrap();

    // Create a simple CalledByEntry-scoped actor (assuming there are accounts
    // inside the wallet).
    let mut a = Actor::new_simple(&c, &w.accounts[0]).unwrap();

    let custom_contract = Uint160::from([9, 8, 7]);
    // Actor has an Invoker inside, so we can perform test invocations, it will
    // have a signer with the first wallet account and CalledByEntry scope.
    let res = a.call(&custom_contract, "method", &[1.into(), 2.into(), 3.into()]).unwrap();
    if res.state != VMState::Halt.to_string() {
        panic!("failed");
    }
    // All of the side-effects in res can be analyzed.

    // Now we want to send the same invocation in a transaction, but we already
    // have the script and a proper system fee for it, therefore SendUncheckedRun
    // can be used.
    let (txid, vub) = a.send_unchecked_run(&res.script, res.gas_consumed, None, None).unwrap();
    let _ = txid;
    let _ = vub;
    // You need to wait for it to persist and then check the on-chain result of it.

    // Now we want to send some transaction, but give it a priority by increasing
    // its network fee, this can be done with Tuned APIs.
    let (txid, vub) = a.send_tuned_call(&custom_contract, "method", None, |r: &Invoke, t: &mut Transaction| {
        // This code is run after the test-invocation done by *Call methods.
        // Reuse the default function to check for HALT execution state.
        let err = DefaultCheckerModifier(r, t);
        if let Some(e) = err {
            return Err(e);
        }
        // Some additional checks can be performed right here, but we only
        // want to raise the network fee by ~20%.
        t.network_fee += t.network_fee / 5;
        Ok(())
    }, &[1.into(), 2.into(), 3.into()]).unwrap();
    let _ = txid;
    let _ = vub;

    // Actor can be used for higher-level wrappers as well, if we want to interact with
    // NEO then [neo] package can accept our Actor and allow to easily use NEO methods.
    let neo_contract = NeoContract::new(&a);
    let balance = neo_contract.balance_of(&a.sender()).unwrap();
    let _ = balance;

    // Now suppose the second wallet account is a committee account. We want to
    // create and sign transactions for committee, but use the first account as
    // a sender (because committee account has no GAS). We at the same time want
    // to make all transactions using this actor high-priority ones, because
    // committee can use this attribute.

    // Get the default options to have CheckerModifier/Modifier set up correctly.
    let mut opts = NewDefaultOptions();
    // And override attributes.
    opts.attributes = vec![Attribute::new(HighPriority)];

    // Create an Actor.
    a = Actor::new_tuned(&c, vec![
        SignerAccount {
            // Sender, regular account with None scope.
            signer: Signer {
                account: w.accounts[0].script_hash(),
                scopes: None,
            },
            account: w.accounts[0].clone(),
        },
        SignerAccount {
            // Committee.
            signer: Signer {
                account: w.accounts[1].script_hash(),
                scopes: CalledByEntry,
            },
            account: w.accounts[1].clone(),
        }
    ], opts).unwrap();

    // Use policy contract wrapper to simplify things. All changes in the
    // Policy contract are made by the committee.
    let policy_contract = PolicyContract::new(&a);

    // Create a transaction to set storage price, it'll be high-priority and have two
    // signers from above. Committee is a multisignature account, so we can't sign/send
    // it right away, w.accounts[1] has only one public key. Therefore, we need to
    // create a partially signed transaction and save it, then collect other signatures
    // and send.
    let tx = policy_contract.set_storage_price_unsigned(10).unwrap();

    let net = a.get_network();
    let mut sc_ctx = ParameterContext::new(TransactionType, net, &tx);
    let sign = w.accounts[0].sign_hashable(net, &tx);
    sc_ctx.add_signature(w.accounts[0].script_hash(), &w.accounts[0].contract, &w.accounts[0].public_key(), &sign).unwrap();

    let sign = w.accounts[1].sign_hashable(net, &tx);
    sc_ctx.add_signature(w.accounts[1].script_hash(), &w.accounts[1].contract, &w.accounts[1].public_key(), &sign).unwrap();

    let data = json!(sc_ctx).to_string();
    let mut file = File::create("tx.json").unwrap();
    file.write_all(data.as_bytes()).unwrap();

    // Signature collection is out of scope, usually it's manual for cases like this.
}
