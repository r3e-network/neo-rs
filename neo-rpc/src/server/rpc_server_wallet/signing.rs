//! C#-compatible wallet signing and relay finalization.
//!
//! Transfer handlers build candidate transactions. This module owns the lower
//! level `Wallet.Sign(ContractParametersContext)` parity flow, witness
//! completion, network-fee adjustment, and relay result projection.

use neo_crypto::{Crypto, ECCurve, ECPoint};
use neo_execution::contract::Contract;
use neo_execution::contract_parameters_context::ContractParametersContext;
use neo_execution::helper::Helper as ContractHelper;
use neo_io::Serializable;
use neo_payloads::transaction::Transaction;
use neo_primitives::UInt160;
use neo_storage::persistence::{CacheRead, DataCache};
use neo_wallets::{Nep6Account, Nep6Wallet, Wallet as CoreWallet, WalletAccount};
use serde_json::Value;
use std::sync::Arc;

use crate::server::contract_state_provider::{
    DeployedContractProvider, DeployedContractProviderFactory,
    NativeDeployedContractProviderFactory,
};
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_relay;
use crate::server::rpc_server::RpcServer;
use crate::server::wallet_compat;

use super::RpcServerWallet;
use super::native_provider::{NativeWalletProvider, WalletNativeProvider};
use super::support::signature_contract_pubkey;

pub(super) fn sign_and_relay<B: CacheRead>(
    server: &RpcServer,
    wallet: &Arc<Nep6Wallet>,
    mut tx: Transaction,
    snapshot_arc: Arc<DataCache<B>>,
) -> Result<Value, RpcException> {
    let mut sign_data: Option<Vec<u8>> = None;

    // Build contract parameter context and add signatures from available keys.
    let mut context = ContractParametersContext::new_with_type(
        snapshot_arc.clone(),
        tx.clone(),
        server.system().settings().network,
        Some("Neo.Network.P2P.Payloads.Transaction".to_string()),
    );
    // Mirror C# `Wallet.Sign(ContractParametersContext)` (Wallet.cs:688-735):
    // iterate the script hashes to be verified and, for each, try the
    // self-contained multi-sig member loop, then single-sig, then the
    // deployed-contract (AddWithScriptHash) fallback.
    let signer_accounts: Vec<UInt160> = tx.signers().iter().map(|signer| signer.account).collect();
    for signer_account in signer_accounts {
        let mut handled = false;
        if let Some(account) = wallet.account(&signer_account) {
            if !account.is_locked() {
                // Try to sign self-contained multi-sig (Wallet.cs:700-719).
                if let Some(contract) = account.contract() {
                    if let Some((mut m, member_points)) =
                        ContractHelper::parse_multi_sig_contract(&contract.script)
                    {
                        let multisig_contract = Contract::create(
                            contract.parameter_list.clone(),
                            contract.script.clone(),
                        );
                        for member_bytes in &member_points {
                            let Some(member_point) = member_point(member_bytes) else {
                                continue;
                            };
                            let Some(member_account) = account_for_point(wallet, &member_point)
                            else {
                                continue;
                            };
                            if !member_account.has_key() || member_account.is_locked() {
                                continue; // check `Lock` or not? (Wallet.cs:708)
                            }
                            let Some(key) = member_account.key() else {
                                continue;
                            };
                            let signature = wallet_compat::sign_transaction_with_key(
                                &tx,
                                &key,
                                server.system().settings().network,
                            )
                            .map_err(internal_error)?;
                            let pub_key =
                                ECPoint::new(ECCurve::Secp256r1, key.compressed_public_key())
                                    .map_err(|e| internal_error(e.to_string()))?;
                            let ok = context
                                .add_signature(multisig_contract.clone(), pub_key, signature)
                                .map_err(|e| internal_error(e.to_string()))?;
                            if ok {
                                m -= 1;
                            }
                            if context.completed() || m == 0 {
                                break;
                            }
                        }
                        handled = true;
                    }
                }

                // Try to sign with a regular (single-sig) account (Wallet.cs:720-727).
                if !handled {
                    let mut contract_opt: Option<Contract> = account
                        .contract()
                        .cloned()
                        .map(|c| Contract::create(c.parameter_list, c.script));
                    let key_opt = account.key();
                    if contract_opt.is_none() {
                        if let Some(ref key) = key_opt {
                            let pub_point = key
                                .public_key_point()
                                .ok()
                                .and_then(|p| ECPoint::from_bytes(&p.to_bytes()).ok());
                            if let Some(point) = pub_point {
                                contract_opt = Some(Contract::create_signature_contract(point));
                            }
                        }
                    }

                    if let Some(contract) = contract_opt {
                        if let Some(key) = key_opt {
                            if account.has_key() {
                                let signature = wallet_compat::sign_transaction_with_key(
                                    &tx,
                                    &key,
                                    server.system().settings().network,
                                )
                                .map_err(internal_error)?;
                                // Neo N3 uses secp256r1 (NIST P-256) curve.
                                let pub_key =
                                    ECPoint::new(ECCurve::Secp256r1, key.compressed_public_key())
                                        .map_err(|e| internal_error(e.to_string()))?;
                                let _ = context.add_signature(contract.clone(), pub_key, signature);
                                handled = true;
                            }
                        } else if account.has_key() {
                            let sign_data = if let Some(data) = sign_data.as_ref() {
                                data.clone()
                            } else {
                                let data = neo_payloads::get_sign_data_vec(
                                    &tx,
                                    server.system().settings().network,
                                )
                                .map_err(|err| internal_error(err.to_string()))?;
                                sign_data = Some(data.clone());
                                data
                            };
                            let wallet_clone = Arc::clone(wallet);
                            let signature =
                                RpcServerWallet::await_wallet_future(Box::pin(async move {
                                    wallet_clone.sign(&sign_data, &signer_account).await
                                }))?;
                            if signature.len() != 64 {
                                return Err(internal_error(
                                    "Invalid signature length from wallet".to_string(),
                                ));
                            }
                            let pub_key_bytes = signature_contract_pubkey(&contract.script)?;
                            let pub_key = ECPoint::new(ECCurve::Secp256r1, pub_key_bytes)
                                .map_err(|e| internal_error(e.to_string()))?;
                            let _ = context.add_signature(contract.clone(), pub_key, signature);
                            handled = true;
                        }
                    }
                }
            }
        }

        // Try smart-contract verification (Wallet.cs:731,
        // ContractParametersContext.AddWithScriptHash).
        if !handled {
            add_with_script_hash(&mut context, snapshot_arc.as_ref(), &signer_account);
        }
    }

    if !context.completed() {
        return Ok(context.to_json());
    }

    if let Some(witnesses) = context.witnesses() {
        tx.set_witnesses(witnesses);
    }

    // Adjust network fee if necessary (parity with C# min fee calculation).
    if tx.size() > 1024 {
        let fee_per_byte = NativeWalletProvider::new(server.system().native_contract_provider())
            .fee_per_byte(snapshot_arc.as_ref())
            .map(i64::from)
            .map_err(internal_error)?;
        let cal_fee = tx.size() as i64 * fee_per_byte + 100_000;
        if tx.network_fee() < cal_fee {
            tx.set_network_fee(cal_fee);
        }
    }
    if tx.network_fee() > server.settings().max_fee {
        return Err(RpcException::from(RpcError::wallet_fee_limit()));
    }

    match rpc_relay::relay_transaction(server, tx.clone()) {
        Ok(relay_result) => {
            rpc_relay::map_relay_result(relay_result)?;
            let settings = server.system().settings();
            Ok(tx.to_json(&settings))
        }
        Err(err) => {
            // Preverify failure: surface unsigned context.
            let mut context = ContractParametersContext::new_with_type(
                snapshot_arc,
                tx.clone(),
                server.system().settings().network,
                Some("Neo.Network.P2P.Payloads.Transaction".to_string()),
            );
            for signer in tx.signers() {
                if let Some(account) = wallet.account(&signer.account) {
                    if let Some(contract) = account.contract() {
                        context.add_contract(Contract::create(
                            contract.parameter_list.clone(),
                            contract.script.clone(),
                        ));
                    }
                }
            }
            let mut json = context.to_json();
            if let Some(obj) = json.as_object_mut() {
                obj.insert("preverifyFail".to_string(), Value::String(err.to_string()));
            }
            Ok(json)
        }
    }
}

/// Builds an `ECPoint` from a multi-sig member's compressed public-key bytes.
fn member_point(member_bytes: &[u8]) -> Option<ECPoint> {
    ECPoint::new(ECCurve::Secp256r1, member_bytes.to_vec()).ok()
}

/// Looks up the wallet account for a member public key, mirroring C#
/// `Wallet.GetAccount(ECPoint)` which resolves the account by the script
/// hash of the single-signature redeem script for that key (Wallet.cs:305-308).
fn account_for_point(wallet: &Arc<Nep6Wallet>, point: &ECPoint) -> Option<Arc<Nep6Account>> {
    let script = Contract::create_signature_redeem_script(point.clone());
    let script_hash = UInt160::from(Crypto::hash160(&script));
    wallet.account(&script_hash)
}

/// Mirrors C# `ContractParametersContext.AddWithScriptHash`
/// (ContractParametersContext.cs:253-268): if a deployed contract exists for
/// the script hash and its `verify` method takes no parameters, add it as a
/// parameterless witness so the transaction can be verified by the contract.
fn add_with_script_hash<B: CacheRead>(
    context: &mut ContractParametersContext,
    snapshot: &DataCache<B>,
    script_hash: &UInt160,
) {
    let contract = match NativeDeployedContractProviderFactory
        .provider()
        .contract_state_by_hash(snapshot, script_hash)
    {
        Ok(Some(contract)) => contract,
        _ => return,
    };
    // C# `DeployedContract.ParameterList` is derived from the `verify`
    // method's ABI parameters; AddWithScriptHash only works with a
    // parameterless verify. `DeployedContract` carries the deployed hash and
    // an empty script (the witness invokes the contract by hash), so build a
    // hash-only contract with an empty parameter list.
    if let Some(verify) = contract.manifest.abi.get_method_ref("verify", 0) {
        if verify.parameters.is_empty() {
            context.add_contract(Contract::create_with_hash(*script_hash, Vec::new()));
        }
    }
}
