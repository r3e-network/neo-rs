use std::time::Duration;
use std::thread::sleep;
use std::sync::Arc;
use std::str::FromStr;

use bigdecimal::BigDecimal;
use tokio::runtime::Runtime;
use tokio::time::sleep as tokio_sleep;

use neo_core2::rpcclient::{RpcClient, RpcClientOptions};
use neo_core2::rpcclient::actor::{Actor, SimpleActor};
use neo_core2::rpcclient::gas::Gas;
use neo_core2::rpcclient::notary::{Notary, ActorOptions, OnNEP17PaymentData};
use neo_core2::rpcclient::policy::Policy;
use neo_core2::vm::vmstate::VMState;
use neo_core2::wallet::Wallet;
use neo_core2::core::transaction::{Transaction, Attribute, HighPriority, Signer, None, CalledByEntry};

fn example_actor() {
    // No error checking done at all, intentionally.
    let mut w = Wallet::new_from_file("somewhere").unwrap();
    // We assume there are two accounts in the wallet --- one is a simple signature
    // account and another one is committee account. The first one will send notary
    // requests, while committee signatures need to be collected.

    // Create an RPC client.
    let rt = Runtime::new().unwrap();
    let c = Arc::new(RpcClient::new("url", RpcClientOptions::default()));

    // An actor for the first account.
    let single = SimpleActor::new(c.clone(), w.accounts[0].clone());

    // Transfer some GAS to the Notary contract to be able to send notary requests
    // from the first account.
    let gas_single = Gas::new(single.clone());
    let (txid, vub) = gas_single.transfer(single.sender(), Notary::hash(), BigDecimal::from_str("1000000000").unwrap(), &OnNEP17PaymentData { till: 10000000 }).unwrap();

    let mut deposit_ok = false;
    // Wait for transaction to be persisted, either it gets in and we get
    // an application log with some result or it expires.
    rt.block_on(async {
        loop {
            let height = c.get_block_count().await.unwrap();
            if height > vub {
                break;
            }
            let app_log = c.get_application_log(txid.clone(), None).await;
            // We can't separate "application log missing" from other errors at the moment, see #2248.
            match app_log {
                Ok(log) => {
                    if log.executions.len() == 1 && log.executions[0].vm_state == VMState::Halt {
                        deposit_ok = true;
                        break;
                    } else {
                        break;
                    }
                }
                Err(_) => {
                    tokio_sleep(Duration::from_secs(5)).await;
                }
            }
        }
    });
    if !deposit_ok {
        panic!("deposit failed");
    }

    let mut opts = ActorOptions::default();
    // Add high priority attribute, we gonna be making committee-signed transactions anyway.
    opts.main_attributes = vec![Attribute { attr_type: HighPriority }];

    // Create an Actor with the simple account used for paying fees and committee
    // signature to be collected.
    let multi = Notary::new_tuned_actor(c.clone(), vec![
        SignerAccount {
            signer: Signer {
                account: w.accounts[0].script_hash(),
                scopes: None,
            },
            account: w.accounts[0].clone(),
        },
        SignerAccount {
            signer: Signer {
                account: w.accounts[1].script_hash(),
                scopes: CalledByEntry,
            },
            account: w.accounts[1].clone(),
        },
    ], opts);

    // Use the Policy contract to perform something requiring committee signature.
    let policy_contract = Policy::new(multi.clone());

    // Wrap a transaction to set storage price into a notary request. Fallback will
    // be create automatically and all appropriate attributes will be added to both
    // transactions.
    let (main_tx, fb_tx, vub) = multi.notarize(policy_contract.set_storage_price_transaction(10)).unwrap();
    let _ = main_tx;
    let _ = fb_tx;
    let _ = vub;
}
