use super::*;
impl RpcServerWallet {
    pub(super) fn get_wallet_balance(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let asset = parse_uint160(params, 0, "getwalletbalance")?;
        let wallet = Self::require_wallet(server)?;
        if asset == NeoToken::new().hash() {
            let token = NeoToken::new();
            let store = server.system().store_cache();
            let mut total = BigInt::zero();
            for account in wallet.get_accounts() {
                if !account.has_key() {
                    continue;
                }
                let amount = token
                    .balance_of_snapshot(&store, &account.script_hash())
                    .map_err(|err| internal_error(err.to_string()))?;
                total += amount;
            }
            let display = BigDecimal::new(total, token.decimals());
            return Ok(json!({ "balance": display.to_string() }));
        }
        if asset == GasToken::new().hash() {
            let token = GasToken::new();
            let store = server.system().store_cache();
            let mut total = BigInt::zero();
            for account in wallet.get_accounts() {
                if !account.has_key() {
                    continue;
                }
                let amount = token.balance_of_snapshot(&store, &account.script_hash());
                total += amount;
            }
            let display = BigDecimal::new(total, token.decimals());
            return Ok(json!({ "balance": display.to_string() }));
        }

        let balance = Self::calculate_nep17_balance(server, &wallet, &asset)?;
        Ok(json!({ "balance": balance.to_string() }))
    }

    pub(super) fn get_wallet_unclaimed_gas(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        let wallet = Self::require_wallet(server)?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let height = ledger
            .current_index(&store)
            .map_err(|err| internal_error(err.to_string()))?
            .saturating_add(1);
        let neo = NeoToken::new();
        let mut total = BigInt::zero();
        for account in wallet.get_accounts() {
            let gas = neo
                .unclaimed_gas(&store, &account.script_hash(), height)
                .map_err(|err| internal_error(err.to_string()))?;
            total += gas;
        }
        Ok(Value::String(total.to_string()))
    }

    pub(super) fn list_address(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        let wallet = Self::require_wallet(server)?;
        let entries = wallet
            .get_accounts()
            .into_iter()
            .map(|account| Self::account_to_json(&account))
            .collect::<Vec<_>>();
        Ok(Value::Array(entries))
    }

    pub(super) fn account_to_json(account: &Arc<dyn WalletAccount>) -> Value {
        let has_key = account.has_key();
        let mut map = Map::new();
        map.insert("address".to_string(), Value::String(account.address()));
        map.insert("haskey".to_string(), Value::Bool(has_key));
        map.insert(
            "label".to_string(),
            account
                .label()
                .map_or(Value::Null, |label| Value::String(label.to_string())),
        );
        map.insert("watchonly".to_string(), Value::Bool(!has_key));
        Value::Object(map)
    }

    fn calculate_nep17_balance(
        server: &RpcServer,
        wallet: &Arc<dyn CoreWallet>,
        asset: &UInt160,
    ) -> Result<BigDecimal, RpcException> {
        let accounts: Vec<UInt160> = wallet
            .get_accounts()
            .into_iter()
            .filter(|account| account.has_key())
            .map(|account| account.script_hash())
            .collect();
        if accounts.is_empty() {
            return Ok(Self::zero_balance());
        }

        let script = Self::build_balance_script(asset, &accounts)?;
        let store = server.system().store_cache();
        let snapshot = Arc::new(store.data_cache().clone());
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            snapshot,
            None,
            server.system().settings().clone(),
            server.settings().max_gas_invoke,
            None,
            neo_native_contracts::native::composition::default_native_services(),
        )
        .map_err(|err| internal_error(err.to_string()))?;
        engine
            .load_script(script, CallFlags::READ_ONLY, Some(*asset))
            .map_err(|err| internal_error(err.to_string()))?;
        engine
            .execute()
            .map_err(|err| internal_error(err.to_string()))?;
        if engine.state() == VMState::FAULT {
            return Ok(Self::zero_balance());
        }
        let decimals_value = engine
            .peek(0)
            .map_err(internal_error)?
            .as_int()
            .map_err(|err| internal_error(err.to_string()))?;
        let decimals = decimals_value
            .to_u8()
            .ok_or_else(|| invalid_params("invalid decimals value"))?;
        let amount_value = engine
            .peek(1)
            .map_err(internal_error)?
            .as_int()
            .map_err(|err| internal_error(err.to_string()))?;
        Ok(BigDecimal::new(amount_value, decimals))
    }

    fn build_balance_script(
        asset: &UInt160,
        accounts: &[UInt160],
    ) -> Result<Vec<u8>, RpcException> {
        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(0);
        for account in accounts {
            let account_bytes = account.to_bytes();
            Self::emit_contract_call(
                &mut builder,
                asset,
                "balanceOf",
                &[account_bytes.as_slice()],
                CallFlags::READ_ONLY,
            )?;
            builder.emit_opcode(OpCode::ADD);
        }
        Self::emit_contract_call(&mut builder, asset, "decimals", &[], CallFlags::READ_ONLY)?;
        Ok(builder.to_array())
    }

    fn emit_contract_call(
        builder: &mut ScriptBuilder,
        contract: &UInt160,
        method: &str,
        args: &[&[u8]],
        flags: CallFlags,
    ) -> Result<(), RpcException> {
        if args.is_empty() {
            builder.emit_opcode(OpCode::NEWARRAY0);
        } else {
            for arg in args.iter().rev() {
                builder.emit_push(arg);
            }
            builder.emit_push_int(args.len() as i64);
            builder.emit_opcode(OpCode::PACK);
        }

        builder.emit_push_int(i64::from(flags.bits()));
        builder.emit_push(method.as_bytes());
        let hash_bytes = contract.to_bytes();
        builder.emit_push(&hash_bytes);
        builder
            .emit_syscall("System.Contract.Call")
            .map_err(|err| internal_error(err.to_string()))?;
        Ok(())
    }

    fn zero_balance() -> BigDecimal {
        BigDecimal::new(BigInt::zero(), 0)
    }
}
