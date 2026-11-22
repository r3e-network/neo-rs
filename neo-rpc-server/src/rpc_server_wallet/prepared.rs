use super::*;

/// Queries NEP-17 contract metadata (decimals and symbol).
///
/// Corresponds to C# RpcServer.AssetDescriptor class.
/// Executes the contract's `decimals()` and `symbol()` methods using ApplicationEngine.
///
/// Reference: neo-project/neo-modules RpcServer.Wallet.cs
pub(super) struct AssetDescriptor {
    pub(super) decimals: u8,
    _symbol: String,
}

impl AssetDescriptor {
    pub(super) fn new(
        cache: &PersistenceDataCache,
        settings: &Arc<neo_protocol::ProtocolSettings>,
        asset: UInt160,
    ) -> Result<Self, String> {
        // Handle native contracts (NEO and GAS) directly
        if asset == NeoToken::new().hash() {
            return Ok(Self {
                decimals: NeoToken::new().decimals(),
                _symbol: "NEO".to_string(),
            });
        }
        if asset == GasToken::new().hash() {
            return Ok(Self {
                decimals: GasToken::new().decimals(),
                _symbol: "GAS".to_string(),
            });
        }

        // Query NEP-17 contract via ApplicationEngine
        Self::query_nep17_metadata(cache, settings, &asset)
    }

    pub(super) fn query_nep17_metadata(
        cache: &PersistenceDataCache,
        settings: &Arc<neo_protocol::ProtocolSettings>,
        asset: &UInt160,
    ) -> Result<Self, String> {
        // Build script to call decimals() and symbol() methods
        let mut script = ScriptBuilder::new();

        // Call decimals()
        script.emit_opcode(OpCode::PUSH0); // empty args array
        script.emit_opcode(OpCode::NEWARRAY);
        script.emit_push_int(i64::from(CallFlags::READ_ONLY.bits()));
        script.emit_push_string("decimals");
        script.emit_push_byte_array(&asset.to_bytes());
        script
            .emit_syscall("System.Contract.Call")
            .map_err(|e| format!("Failed to build decimals call: {e}"))?;

        // Call symbol()
        script.emit_opcode(OpCode::PUSH0); // empty args array
        script.emit_opcode(OpCode::NEWARRAY);
        script.emit_push_int(i64::from(CallFlags::READ_ONLY.bits()));
        script.emit_push_string("symbol");
        script.emit_push_byte_array(&asset.to_bytes());
        script
            .emit_syscall("System.Contract.Call")
            .map_err(|e| format!("Failed to build symbol call: {e}"))?;

        // Execute the script
        let snapshot = Arc::new(cache.clone());
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            snapshot,
            None,
            settings.as_ref().clone(),
            10_000_000, // 0.1 GAS gas limit for metadata query
            None,
            neo_native_contracts::native::composition::default_native_services(),
        )
        .map_err(|e| format!("Failed to create engine: {e}"))?;

        engine
            .load_script(script.to_array(), CallFlags::READ_ONLY, Some(*asset))
            .map_err(|e| format!("Failed to load script: {e}"))?;

        engine
            .execute()
            .map_err(|e| format!("Failed to execute script: {e}"))?;

        if engine.state() != VMState::HALT {
            return Err(format!(
                "Contract execution failed (state: {:?}). Asset may not be a valid NEP-17 token.",
                engine.state()
            ));
        }

        // Read results from stack (symbol is on top, decimals below)
        let symbol_item = engine
            .peek(0)
            .map_err(|e| format!("Failed to read symbol from stack: {e}"))?;
        let decimals_item = engine
            .peek(1)
            .map_err(|e| format!("Failed to read decimals from stack: {e}"))?;

        // Parse decimals
        let decimals_bigint = decimals_item
            .as_int()
            .map_err(|e| format!("decimals() did not return an integer: {e}"))?;
        let decimals = decimals_bigint
            .to_u8()
            .ok_or_else(|| format!("decimals value out of range: {decimals_bigint}"))?;

        // Parse symbol
        let symbol_bytes = symbol_item
            .as_bytes()
            .map_err(|e| format!("symbol() did not return a byte array: {e}"))?;
        let symbol = String::from_utf8(symbol_bytes)
            .map_err(|e| format!("symbol() returned invalid UTF-8: {e}"))?;

        if symbol.is_empty() {
            return Err("symbol() returned empty string".to_string());
        }

        Ok(Self {
            decimals,
            _symbol: symbol,
        })
    }
}

/// Type name the RPC wallet reports for a transaction context, matching C#.
pub(super) const TRANSACTION_TYPE_NAME: &str = "Neo.Network.P2P.Payloads.Transaction";

/// Local ledger answers for RPC wallet authorization.
///
/// Every method reads the snapshot in place, so each port future is ready on
/// its first poll and the synchronous RPC handlers can drive it without a
/// reactor. This is the server-side counterpart of the client's RPC port: both
/// feed the same [`TransactionAuthorizer`].
pub(super) struct LedgerChainQuery {
    snapshot: PersistenceDataCache,
}

impl LedgerChainQuery {
    pub(super) fn new(snapshot: PersistenceDataCache) -> Self {
        Self { snapshot }
    }
}

#[async_trait::async_trait]
impl TransactionChainQuery for LedgerChainQuery {
    type Error = String;

    async fn block_height(&self) -> Result<u32, Self::Error> {
        LedgerContract::new()
            .current_index(&self.snapshot)
            .map_err(|err| err.to_string())
    }

    async fn gas_balance(&self, account: &UInt160) -> Result<BigInt, Self::Error> {
        Ok(GasToken::new().balance_of_snapshot(&self.snapshot, account))
    }

    async fn network_fee(&self, transaction: &Transaction) -> Result<i64, Self::Error> {
        // The node prices a transaction from its signed witnesses at relay
        // time; wallet preparation validates the caller-supplied fee against
        // the policy floor and `MaxFee` instead of re-estimating it here.
        Ok(transaction.network_fee())
    }
}

/// Renders an authorization context as the JSON the wallet RPC returns.
///
/// Signature slots, requirement counts and completion are read from the shared
/// [`TransactionAuthorizer`]; only the wire shape is owned by the RPC layer.
pub(super) fn authorization_json(
    authorizer: &TransactionAuthorizer,
    network: u32,
    type_name: Option<&str>,
) -> Value {
    let items: Vec<Value> = authorizer
        .entries()
        .map(|(script_hash, context)| {
            let signatures: Vec<Value> = context
                .collected_signatures()
                .into_iter()
                .map(|(public_key, signature)| {
                    json!({
                        "publicKey": hex::encode(public_key),
                        "signature": hex::encode(signature)
                    })
                })
                .collect();

            json!({
                "scriptHash": script_hash.to_string(),
                "required": context.required(),
                "collected": context.collected(),
                "signatures": signatures
            })
        })
        .collect();

    json!({
        "type": type_name.unwrap_or(TRANSACTION_TYPE_NAME),
        "hash": authorizer.tx().hash().to_string(),
        "network": network,
        "items": items,
        "completed": authorizer.is_complete()
    })
}
#[derive(Debug)]
pub(super) struct PreparedTransaction {
    /// Authorization state shared with `neo-rpc-client`.
    pub(super) authorizer: TransactionAuthorizer,
    /// Unsigned serialization the signatures are produced against.
    pub(super) unsigned_data: Vec<u8>,
}

impl PreparedTransaction {
    pub(super) fn transaction(&self) -> &Transaction {
        self.authorizer.tx()
    }

    pub(super) fn complete(mut self, witnesses: Vec<Witness>) -> Result<Transaction, RpcException> {
        self.authorizer.install_witnesses(witnesses);
        let current_unsigned_data = self
            .authorizer
            .tx()
            .try_get_hash_data()
            .map_err(|err| internal_error(err.to_string()))?;
        if current_unsigned_data != self.unsigned_data {
            return Err(internal_error(
                "Transaction hash fields changed after authorization; signatures were discarded"
                    .to_string(),
            ));
        }
        Ok(self.authorizer.tx().clone())
    }
}

pub(super) enum WalletAuthorization {
    Signed(Transaction),
    Pending(Value),
}
