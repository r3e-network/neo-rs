//! Account selection helpers for C# wallet transaction construction.

use neo_payloads::signer::Signer;
use neo_primitives::{UInt160, WitnessScope};
use neo_wallets::Wallet;
use num_bigint::BigInt;
use num_traits::Zero;

pub(super) fn spendable_wallet_accounts(
    wallet: &dyn Wallet,
    sender: Option<UInt160>,
) -> Vec<UInt160> {
    match sender {
        Some(sender) => vec![sender],
        None => wallet
            .get_accounts()
            .into_iter()
            .filter(|account| !account.is_locked() && account.has_key())
            .map(|account| account.script_hash())
            .collect(),
    }
}

/// C# `Wallet.GetSigners(sender, cosigners)`: moves the sender's signer
/// to the front, or prepends a `WitnessScope::NONE` sender signer.
pub(super) fn get_signers(sender: UInt160, cosigners: &[Signer]) -> Vec<Signer> {
    for (i, cosigner) in cosigners.iter().enumerate() {
        if cosigner.account == sender {
            if i == 0 {
                return cosigners.to_vec();
            }
            let mut list = cosigners.to_vec();
            let signer = list.remove(i);
            list.insert(0, signer);
            return list;
        }
    }
    let mut list = Vec::with_capacity(cosigners.len() + 1);
    list.push(Signer::new(sender, WitnessScope::NONE));
    list.extend_from_slice(cosigners);
    list
}

/// C# `Wallet.FindPayingAccounts(orderedAccounts, amount)`.
pub(super) fn find_paying_accounts(
    ordered_accounts: &mut Vec<(UInt160, BigInt)>,
    mut amount: BigInt,
) -> Vec<(UInt160, BigInt)> {
    let mut result = Vec::new();
    let sum_balance: BigInt = ordered_accounts.iter().map(|(_, value)| value).sum();
    if sum_balance == amount {
        result.append(ordered_accounts);
        return result;
    }

    for i in 0..ordered_accounts.len() {
        if ordered_accounts[i].1 < amount {
            continue;
        }
        if ordered_accounts[i].1 == amount {
            result.push(ordered_accounts.remove(i));
        } else {
            result.push((ordered_accounts[i].0, amount.clone()));
            ordered_accounts[i].1 -= amount.clone();
        }
        break;
    }
    if result.is_empty() && !ordered_accounts.is_empty() {
        let mut i = ordered_accounts.len() - 1;
        while ordered_accounts[i].1 <= amount {
            let (account, value) = ordered_accounts.remove(i);
            amount -= value.clone();
            result.push((account, value));
            if i == 0 {
                break;
            }
            i -= 1;
        }
        if amount > BigInt::zero() {
            for i in 0..ordered_accounts.len() {
                if ordered_accounts[i].1 < amount {
                    continue;
                }
                if ordered_accounts[i].1 == amount {
                    result.push(ordered_accounts.remove(i));
                } else {
                    result.push((ordered_accounts[i].0, amount.clone()));
                    ordered_accounts[i].1 -= amount.clone();
                }
                break;
            }
        }
    }
    result
}
