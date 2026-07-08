//! Notary native-method handlers.
//!
//! Keeps deposit, withdrawal, verification, and committee-gated setting bodies
//! out of the contract root while preserving C#-compatible validation order,
//! storage writes, witness checks, fee accounting, and signature verification.
//! Dispatch is declared by the metadata binding table and
//! `native_contract_dispatch!`.

use super::Notary;
use crate::{GasToken, LedgerContract, Role, RoleManagement};
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_execution::application_engine_contract::NativeArgNullMask;
use neo_payloads::{Transaction, get_sign_data};
use neo_primitives::{TransactionAttributeType, WitnessScope};
use num_bigint::BigInt;

impl Notary {
    pub(super) fn invoke_get_max_not_valid_before_delta(
        &self,
        engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let delta = self.read_max_not_valid_before_delta(&snapshot)?;
        Ok(BigInt::from(delta).to_signed_bytes_le())
    }

    pub(super) fn invoke_balance_of(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let account = Self::parse_account(args, "balanceOf")?;
        Ok(self
            .read_deposit_field(&snapshot, &account, 0)?
            .to_signed_bytes_le())
    }

    pub(super) fn invoke_expiration_of(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let account = Self::parse_account(args, "expirationOf")?;
        Ok(self
            .read_deposit_field(&snapshot, &account, 1)?
            .to_signed_bytes_le())
    }

    pub(super) fn invoke_lock_deposit_until(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        // C#: CheckWitnessInternal(account) (false return on no witness),
        // then till >= currentIndex+2, an existing deposit, and till not
        // shortening it; on success update Deposit.Till and write back.
        let account = Self::parse_account(args, "lockDepositUntil")?;
        let till = crate::args::raw_u32_arg(args, 1, "Notary::lockDepositUntil").map_err(|_| {
            CoreError::invalid_operation("Notary::lockDepositUntil requires a uint till")
        })?;
        // CheckWitnessInternal: a missing witness returns false (not a fault).
        let witnessed = engine
            .check_witness(&account)
            .map_err(|e| CoreError::invalid_operation(format!("lockDepositUntil witness: {e}")))?;
        if !witnessed {
            return Ok(vec![0]);
        }
        let current = LedgerContract::new().current_index(&snapshot)?;
        let deposit = self.read_deposit(&snapshot, &account)?;
        match Self::lock_deposit_decision(current, deposit, till) {
            Some((amount, new_till)) => {
                self.write_deposit(&engine.snapshot_cache(), &account, &amount, new_till)?;
                Ok(vec![1])
            }
            None => Ok(vec![0]),
        }
    }

    pub(super) fn invoke_on_nep17_payment(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        // C#: only GAS may deposit; data = Array[to?, till]; the deposit
        // owner (tx.Sender == to) may set the lock height.
        let from = Self::parse_account(args, crate::NEP17_PAYMENT_METHOD)?;
        let amount =
            crate::args::raw_required_integer_arg(args, 1, "Notary::onNEP17Payment", "an amount")?;
        let data = args.get(2).map(Vec::as_slice).unwrap_or(&[]);

        if engine.get_calling_script_hash() != Some(GasToken::script_hash()) {
            return Err(CoreError::invalid_operation(
                "Notary::onNEP17Payment: only GAS can be accepted for deposit",
            ));
        }
        let (to, till) = Self::parse_onnep17_data(&from, data)?;
        // C# `allowedChangeTill = tx.Sender == to`; the script container is
        // the persisting transaction (the GAS transfer that triggered this).
        let sender = engine
            .script_container()
            .and_then(|c| c.as_any().downcast_ref::<Transaction>())
            .and_then(|tx| tx.sender());
        let allowed_change_till = sender == Some(to);

        let current = LedgerContract::new().current_index(&snapshot)?;
        let fee_per_key = crate::PolicyContract::new().attribute_fee(
            &snapshot,
            TransactionAttributeType::NotaryAssisted.to_byte(),
            true,
        )?;
        let existing = self.read_deposit(&snapshot, &to)?;
        match Self::compute_deposit(
            existing,
            &amount,
            till,
            allowed_change_till,
            current,
            fee_per_key,
        ) {
            Ok((new_amount, new_till)) => {
                self.write_deposit(&engine.snapshot_cache(), &to, &new_amount, new_till)?;
                Ok(Vec::new())
            }
            Err(msg) => Err(CoreError::invalid_operation(format!(
                "Notary::onNEP17Payment: {msg}"
            ))),
        }
    }

    pub(super) fn invoke_withdraw(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        // C# Withdraw(from, to?): witness the depositor, then transfer the
        // unlocked deposit GAS from Notary to `to` (defaulting to `from`).
        let from = Self::parse_account(args, "withdraw")?;
        // `to` is a nullable UInt160?: a Null arg (bit 1 of the native arg
        // null-mask) means "send to `from`".
        let to_is_null = engine
            .get_state::<NativeArgNullMask>()
            .is_some_and(|mask| mask.0 & (1 << 1) != 0);
        let receive = if to_is_null {
            from
        } else {
            crate::args::raw_hash160(args, 1, "Notary::withdraw")?
        };

        let witnessed = engine
            .check_witness(&from)
            .map_err(|e| CoreError::invalid_operation(format!("withdraw witness: {e}")))?;
        if !witnessed {
            return Ok(vec![0]);
        }
        let Some((amount, till)) = self.read_deposit(&snapshot, &from)? else {
            return Ok(vec![0]); // no deposit
        };
        if LedgerContract::new().current_index(&snapshot)? < till {
            return Ok(vec![0]); // still locked
        }
        // C# removes the deposit BEFORE the transfer; a failed transfer
        // throws, which rolls back this delete with the rest of the call.
        self.delete_deposit(&engine.snapshot_cache(), &from);
        let notary_hash = Notary::script_hash();
        // from == caller == Notary, so the transfer's witness check passes
        // (Notary moves its own balance), faithful to the C# nested call.
        let ok =
            GasToken::transfer_core(engine, notary_hash, &notary_hash, &receive, &amount, &[])?;
        if !ok {
            return Err(CoreError::invalid_operation(format!(
                "Notary::withdraw: transfer to {receive} failed"
            )));
        }
        Ok(vec![1])
    }

    pub(super) fn invoke_verify(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        // C# Verify(engine, byte[] signature): the script container must
        // be a Transaction carrying a NotaryAssisted attribute whose
        // Notary-account signer (when present) has WitnessScope.None; a
        // Notary-paid transaction (Sender == Hash) must have exactly
        // [Notary, payer] signers with the payer's deposit covering
        // SystemFee + NetworkFee; finally `signature` must be a valid
        // secp256r1 signature over the tx sign-data (network magic ++
        // tx hash) by ONE of the designated P2PNotary nodes. Every
        // rejection returns false, never a fault.
        let signature_is_null = engine
            .get_state::<NativeArgNullMask>()
            .is_some_and(|mask| mask.0 & 1 != 0);
        let signature = args.first().map(Vec::as_slice).unwrap_or(&[]);
        if signature_is_null || signature.len() != 64 {
            return Ok(vec![0]);
        }
        let Some(tx) = engine
            .script_container()
            .and_then(|c| c.as_any().downcast_ref::<Transaction>())
        else {
            return Ok(vec![0]); // C# `engine.ScriptContainer as Transaction` null
        };
        if tx
            .get_attribute(TransactionAttributeType::NotaryAssisted)
            .is_none()
        {
            return Ok(vec![0]);
        }
        let notary_hash = Self::script_hash();
        // The Notary-account signer must not request any witness scope.
        for signer in tx.signers() {
            if signer.account == notary_hash {
                if signer.scopes != WitnessScope::NONE {
                    return Ok(vec![0]);
                }
                break;
            }
        }
        // C# `tx.Sender` is `Signers[0].Account`: a signer-less
        // transaction faults there rather than returning false.
        let sender = tx.sender().ok_or_else(|| {
            CoreError::invalid_operation("Notary::verify: transaction has no signers")
        })?;
        if sender == notary_hash {
            // Notary pays the fees: exactly [Notary, payer] signers and
            // a deposit for the payer that covers the transaction fees.
            if tx.signers().len() != 2 {
                return Ok(vec![0]);
            }
            let payer = tx.signers()[1].account;
            let Some((amount, _till)) = self.read_deposit(&snapshot, &payer)? else {
                return Ok(vec![0]);
            };
            // C# `tx.NetworkFee + tx.SystemFee` is unchecked long math.
            let fees = BigInt::from(tx.network_fee().wrapping_add(tx.system_fee()));
            if amount < fees {
                return Ok(vec![0]);
            }
        }
        // C# GetNotaryNodes: the P2PNotary designation effective at
        // Ledger.CurrentIndex + 1.
        let current = LedgerContract::new().current_index(&snapshot)?;
        let notaries = RoleManagement::new().get_designated_by_role_at(
            &snapshot,
            Role::P2PNotary,
            current.wrapping_add(1),
        )?;
        let network = engine.protocol_settings().network;
        let sign_data = get_sign_data(tx, network)?;
        // C# Crypto.VerifySignature returns false (never throws) for a
        // malformed 64-byte signature; map decode errors to false.
        let valid = notaries
            .iter()
            .any(|n| n.verify_signature(&sign_data, signature).unwrap_or(false));
        Ok(vec![u8::from(valid)])
    }

    pub(super) fn invoke_set_max_not_valid_before_delta(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# param is `uint value`: decode as u32 (out-of-range faults like
        // the C# uint parameter binding).
        let value = crate::args::raw_u32_arg(args, 0, "Notary::setMaxNotValidBeforeDelta")
            .map_err(|_| {
                CoreError::invalid_operation(
                    "Notary::setMaxNotValidBeforeDelta requires a uint value",
                )
            })?;
        // C# v3.10.1 bound: value must be <= GetMaxValidUntilBlockIncrement/2
        // and >= engine.ProtocolSettings.ValidatorsCount (was the constant
        // ProtocolSettings.Default.ValidatorsCount = 0). On a network whose
        // ValidatorsCount > 0 this now rejects small deltas the old check
        // let through - a tx-validity divergence.
        let upper =
            crate::PolicyContract::new().system_max_valid_until_block_increment(engine)? / 2;
        let lower = i64::from(engine.protocol_settings().validators_count);
        if i64::from(value) > upper || i64::from(value) < lower {
            return Err(CoreError::invalid_operation(format!(
                "MaxNotValidBeforeDelta cannot be more than {upper} or less than {lower}"
            )));
        }
        crate::committee::assert_committee(engine, "setMaxNotValidBeforeDelta")?;
        self.put_max_not_valid_before_delta(&engine.snapshot_cache(), i64::from(value));
        Ok(Vec::new())
    }
}
