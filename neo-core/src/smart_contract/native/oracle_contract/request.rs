use super::{OracleContract, PendingRequest, MAX_PENDING_PER_URL};
use crate::error::{CoreError as Error, CoreResult as Result};
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::{contract_management::ContractManagement, GasToken};
use crate::UInt256;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

impl OracleContract {
    pub(super) fn request(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 5 {
            return Err(Error::invalid_operation(
                "Invalid argument count".to_string(),
            ));
        }

        let url = String::from_utf8(args[0].clone())
            .map_err(|_| Error::invalid_operation("Invalid URL"))?;
        // C# parity: filter is stored as ByteString("") when caller passes empty string,
        // and as Null when caller passes StackItem.Null. Our Vec<u8> args layer can't
        // tell `Null` from `ByteString("\x00")` (both produce `[0x00]`), so we consult
        // the dispatcher-populated NativeArgNullMask to detect the Null case explicitly.
        // See application_engine_contract.rs NativeArgNullMask.
        let filter_was_null = engine
            .get_state::<crate::smart_contract::application_engine_contract::NativeArgNullMask>()
            .map(|m| (m.0 >> 1) & 1 == 1)
            .unwrap_or(false);
        let filter = if filter_was_null {
            None
        } else {
            Some(
                String::from_utf8(args[1].clone())
                    .map_err(|_| Error::invalid_operation("Invalid filter"))?,
            )
        };
        let callback = String::from_utf8(args[2].clone())
            .map_err(|_| Error::invalid_operation("Invalid callback"))?;
        // `userData` is declared as `Any` in the native ABI, so the dispatcher
        // (application_engine_contract.rs) hands us the BinarySerializer-serialized
        // form of the stack item. C# stores exactly this serialized blob and caps
        // it at MaxUserDataLength=512 (OracleContract.cs:265), so `user_data.len()`
        // here is the serialized byte length we must bound against the cap below.
        let user_data = args[3].clone();
        let gas_for_response = BigInt::from_signed_bytes_le(&args[4])
            .to_i64()
            .ok_or_else(|| Error::invalid_operation("Invalid gas amount: out of i64 range"))?;

        if url.len() > self.config.max_url_length {
            return Err(Error::invalid_operation("URL too long"));
        }
        if let Some(ref f) = filter {
            if f.len() > self.config.max_filter_length {
                return Err(Error::invalid_operation("Filter too long"));
            }
        }
        if callback.is_empty() || callback.len() > self.config.max_callback_length {
            return Err(Error::invalid_operation(
                "Callback name too long".to_string(),
            ));
        }
        if callback.starts_with('_') {
            return Err(Error::invalid_operation(
                "Callback cannot start with underscore".to_string(),
            ));
        }
        if user_data.len() > self.config.max_user_data_length {
            return Err(Error::invalid_operation("User data too long"));
        }
        if gas_for_response < self.config.min_response_gas
            || gas_for_response > self.config.max_response_gas
        {
            return Err(Error::invalid_operation("Invalid gas amount"));
        }

        let calling_contract = engine.get_calling_script_hash().ok_or_else(|| {
            Error::invalid_operation("Oracle request must be invoked by a contract".to_string())
        })?;
        let snapshot_arc = engine.snapshot_cache();
        let snapshot = snapshot_arc.as_ref();
        if !ContractManagement::is_contract(snapshot, &calling_contract)? {
            return Err(Error::invalid_operation(
                "Oracle request must be invoked by a contract".to_string(),
            ));
        }

        let url_hash = self.compute_url_hash(&url);
        if self.read_id_list(snapshot, &url_hash)?.len() >= MAX_PENDING_PER_URL {
            return Err(Error::invalid_operation(
                "There are too many pending responses for this url".to_string(),
            ));
        }

        let original_tx_id = engine
            .script_container()
            .and_then(|container| container.as_any().downcast_ref::<crate::network::p2p::payloads::Transaction>().map(|tx| tx.hash()))
            .unwrap_or_else(UInt256::zero);
        let price = self.get_price_value(snapshot);
        let price_u64 = u64::try_from(price)
            .map_err(|_| Error::invalid_operation("Oracle price cannot be converted to u64"))?;
        engine.add_runtime_fee(price_u64)?;
        let gas_for_response_u64 = u64::try_from(gas_for_response)
            .map_err(|_| Error::invalid_operation("gasForResponse cannot be converted to u64"))?;
        engine.add_runtime_fee(gas_for_response_u64)?;
        GasToken::new().mint(engine, &self.hash, &BigInt::from(gas_for_response), false)?;
        let id = self.next_request_id(snapshot)?;

        let request = PendingRequest {
            id,
            original_tx_id,
            gas_for_response,
            url,
            filter,
            callback_contract: calling_contract,
            callback_method: callback,
            user_data,
        };

        self.write_request(snapshot, &request)?;
        self.append_request_id(snapshot, &url_hash, id)?;
        self.emit_oracle_request(engine, id, calling_contract, &request)?;

        Ok(Vec::new())
    }
}
