use std::env;
use std::process;
use std::str::FromStr;

use neo_core2::rpcclient::{self, RpcClient};
use neo_core2::encoding::address::Address;

fn example() {
    let endpoint = "https://rpc.t5.n3.nspcc.ru:20331";
    let opts = rpcclient::Options::default();

    let mut c = match RpcClient::new(endpoint, opts) {
        Ok(client) => client,
        Err(err) => {
            eprintln!("{}", err);
            process::exit(1);
        }
    };

    if let Err(err) = c.init() {
        eprintln!("{}", err);
        process::exit(1);
    }

    if let Err(err) = c.ping() {
        eprintln!("{}", err);
        process::exit(1);
    }

    let addr = match Address::from_str("NUkaBmzsZq1qdgaHfKrtRUcHNhtVJ2hTpv") {
        Ok(address) => address,
        Err(err) => {
            eprintln!("{}", err);
            process::exit(1);
        }
    };

    let resp = match c.get_nep17_balances(&addr) {
        Ok(response) => response,
        Err(err) => {
            eprintln!("{}", err);
            process::exit(1);
        }
    };

    println!("{}", resp.address);
    println!("{:?}", resp.balances);
}
