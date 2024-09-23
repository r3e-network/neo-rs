use std::str::FromStr;
use neo_core2::rpcclient::{Client, Options};
use neo_core2::rpcclient::actor::SimpleActor;
use neo_core2::rpcclient::gas;
use neo_core2::rpcclient::neo;
use neo_core2::smartcontract::Builder;
use neo_core2::util::Uint160;
use neo_core2::wallet::Wallet;

fn example_builder() {
    // No error checking done at all, intentionally.
    let w = Wallet::new_from_file("somewhere").unwrap();

    let c = Client::new("url", Options::default()).unwrap();

    // Assuming there is one Account inside.
    let a = SimpleActor::new(&c, &w.accounts()[0]);

    let p_key = hex::decode("03d9e8b16bd9b22d3345d6d4cde31be1c3e1d161532e3d0ccecb95ece2eb58336e").unwrap(); // Public key.

    let mut b = Builder::new();
    // Transfer + vote in a single script with each action leaving return value on the stack.
    b.invoke_method(&neo::HASH, "transfer", &[a.sender(), Uint160::from_str("ff").unwrap(), 1.into(), None.into()]);
    b.invoke_method(&neo::HASH, "vote", &[p_key.into()]);
    let script = b.script().unwrap();

    // Actor has an Invoker inside, so we can perform test invocation using the script.
    let res = a.run(&script).unwrap();
    if res.state != "HALT" || res.stack.len() != 2 {
        panic!("failed"); // The script failed completely or didn't return proper number of return values.
    }

    let transfer_result = res.stack[0].try_bool().unwrap();
    let vote_result = res.stack[1].try_bool().unwrap();

    if !transfer_result {
        panic!("transfer failed");
    }
    if !vote_result {
        panic!("vote failed");
    }

    b.reset(); // Copy the old script above if you need it!

    // Multiple transfers of different tokens in a single script. If any of
    // them fails whole script fails.
    b.invoke_with_assert(&neo::HASH, "transfer", &[a.sender(), Uint160::from_str("70").unwrap(), 1.into(), None.into()]);
    b.invoke_with_assert(&gas::HASH, "transfer", &[a.sender(), Uint160::from_str("71").unwrap(), 100000.into(), "data".as_bytes().into()]);
    b.invoke_with_assert(&neo::HASH, "transfer", &[a.sender(), Uint160::from_str("72").unwrap(), 1.into(), None.into()]);
    let script = b.script().unwrap();

    // Now send a transaction with this script via an RPC node.
    let (txid, vub) = a.send_run(&script).unwrap();
    let _ = txid;
    let _ = vub;
}
