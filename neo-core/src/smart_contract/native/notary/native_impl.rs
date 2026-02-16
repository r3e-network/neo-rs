use super::*;
use crate::smart_contract::native::NativeContract;
use std::any::Any;

impl Default for Notary {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeContract for Notary {
    fn id(&self) -> i32 {
        self.id
    }

    fn name(&self) -> &str {
        "Notary"
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn active_in(&self) -> Option<Hardfork> {
        Some(Hardfork::HfEchidna)
    }

    fn activations(&self) -> Vec<Hardfork> {
        vec![Hardfork::HfEchidna, Hardfork::HfFaun]
    }

    fn supported_standards(&self, settings: &ProtocolSettings, block_height: u32) -> Vec<String> {
        let mut standards = vec!["NEP-27".to_string()];
        if settings.is_hardfork_enabled(Hardfork::HfFaun, block_height) {
            standards.push("NEP-30".to_string());
        }
        standards
    }

    fn initialize(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let snapshot = engine.snapshot_cache();
        let key = Self::max_delta_key();
        if snapshot.as_ref().try_get(&key).is_none() {
            snapshot.add(
                key,
                StorageItem::from_bytes(DEFAULT_MAX_NOT_VALID_BEFORE_DELTA.to_le_bytes().to_vec()),
            );
        }
        Ok(())
    }

    fn on_persist(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let block = engine
            .persisting_block()
            .cloned()
            .ok_or_else(|| Error::native_contract("No persisting block available"))?;

        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let mut total_fees: i64 = 0;
        let mut notaries: Option<Vec<crate::cryptography::ECPoint>> = None;

        for tx in &block.transactions {
            if let Some(TransactionAttribute::NotaryAssisted(attr)) =
                tx.get_attribute(TransactionAttributeType::NotaryAssisted)
            {
                if notaries.is_none() {
                    notaries = Some(self.get_notary_nodes(snapshot_ref)?);
                }

                total_fees += i64::from(attr.nkeys) + 1;

                if tx.sender() == Some(self.hash()) && tx.signers().len() >= 2 {
                    let payer = tx.signers()[1].account;
                    let key = Self::deposit_key(&payer);
                    if let Some(item) = snapshot_ref.try_get(&key) {
                        let mut deposit = deserialize_deposit(&item.get_value())?;
                        deposit.amount -= BigInt::from(tx.system_fee() + tx.network_fee());
                        if deposit.amount.is_zero() {
                            snapshot.delete(&key);
                        } else {
                            Self::persist_deposit(&snapshot, key, true, &deposit);
                        }
                    }
                }
            }
        }

        if total_fees == 0 {
            return Ok(());
        }

        let Some(notaries) = notaries else {
            return Ok(());
        };

        if notaries.is_empty() {
            return Err(Error::native_contract(
                "No notary nodes designated".to_string(),
            ));
        }

        let policy = PolicyContract::new();
        let fee_per_key = policy
            .get_attribute_fee_for_type(
                snapshot_ref,
                TransactionAttributeType::NotaryAssisted as u8,
            )
            .map_err(|err| {
                Error::native_contract(format!("Failed to read Notary attribute fee: {}", err))
            })?;

        let notary_count = i64::try_from(notaries.len())
            .map_err(|_| Error::native_contract("Notary node count exceeds i64 capacity"))?;
        let single_reward = total_fees
            .checked_mul(fee_per_key)
            .ok_or_else(|| Error::native_contract("Notary reward overflow"))?
            / notary_count;

        for notary in notaries {
            let account = Contract::create_signature_contract(notary).script_hash();
            GasToken::new().mint(engine, &account, &BigInt::from(single_reward), false)?;
        }

        Ok(())
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        match method {
            "balanceOf" => {
                if args.is_empty() {
                    return Err(Error::native_contract(
                        "balanceOf requires account argument".to_string(),
                    ));
                }
                let account = Self::parse_uint160(&args[0], "Invalid account hash")?;
                let balance = self.balance_of_arc(&snapshot, &account);
                // Return as integer bytes
                Ok(balance.to_signed_bytes_le())
            }
            "expirationOf" => {
                if args.is_empty() {
                    return Err(Error::native_contract(
                        "expirationOf requires account argument".to_string(),
                    ));
                }
                let account = Self::parse_uint160(&args[0], "Invalid account hash")?;
                let expiration = self.expiration_of_arc(&snapshot, &account);
                Ok(expiration.to_le_bytes().to_vec())
            }
            "getMaxNotValidBeforeDelta" => {
                let delta = self.get_max_not_valid_before_delta_arc(&snapshot);
                Ok(delta.to_le_bytes().to_vec())
            }
            "verify" => self.verify(engine, args),
            "onNEP17Payment" => {
                // Handle GAS deposits from users
                // Args: from (UInt160), amount (BigInt), data (optional)
                self.on_nep17_payment(engine, args)
            }
            "lockDepositUntil" => {
                // Extend deposit lock period
                // Args: account (UInt160), till (u32)
                self.lock_deposit_until(engine, args)
            }
            "withdraw" => {
                // Withdraw deposit after expiration
                // Args: from (UInt160), to (UInt160)
                self.withdraw(engine, args)
            }
            "setMaxNotValidBeforeDelta" => {
                // Set max delta (committee only)
                // Args: value (u32)
                self.set_max_not_valid_before_delta(engine, args)
            }
            _ => Err(Error::native_contract(format!(
                "Unknown Notary method: {}",
                method
            ))),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
