//! Wallet lifecycle, key import/export, and address listing handlers.

use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;

use neo_wallets::{KeyPair, Nep6Wallet, Wallet as CoreWallet, WalletAccount, WalletError};
use serde_json::{Map, Value};
use zeroize::Zeroizing;

use super::RpcServerWallet;
use super::request::{
    DumpPrivKeyRequest, ImportPrivKeyRequest, NoParamsRequest, OpenWalletRequest,
};
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::invalid_params;
use crate::server::rpc_server::RpcServer;

impl RpcServerWallet {
    pub(super) fn close_wallet(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "closewallet")?;
        server.set_wallet(None);
        Ok(Value::Bool(true))
    }

    pub(super) fn dump_priv_key(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request =
            DumpPrivKeyRequest::parse(params, server.system().settings().address_version)?;
        let script_hash = request.script_hash;
        let wallet = Self::require_wallet(server)?;
        let account = wallet.account(&script_hash).ok_or_else(|| {
            RpcException::from(RpcError::unknown_account().with_data(script_hash.to_string()))
        })?;
        if !account.has_key() {
            return Err(RpcException::from(
                RpcError::unknown_account().with_data(format!("{script_hash} is watch-only")),
            ));
        }
        let wif = account.export_wif().map_err(|err| {
            RpcException::from(RpcError::internal_server_error().with_data(err.to_string()))
        })?;
        Ok(Value::String(wif))
    }

    pub(super) fn get_new_address(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getnewaddress")?;
        let wallet = Self::require_wallet(server)?;
        let key_pair = KeyPair::generate().map_err(|err| {
            RpcException::from(RpcError::internal_server_error().with_data(err.to_string()))
        })?;
        let wallet_clone = Arc::clone(&wallet);
        let key_bytes = Zeroizing::new(*key_pair.private_key());
        let account = Self::await_wallet_future(Box::pin(async move {
            wallet_clone.create_account(key_bytes.as_ref()).await
        }))?;
        Self::save_wallet(&wallet)?;
        Ok(Value::String(account.address()))
    }

    pub(super) fn import_priv_key(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request = ImportPrivKeyRequest::parse(params)?;
        KeyPair::from_wif(&request.wif)
            .map_err(|err| invalid_params(format!("invalid WIF: {err}")))?;
        let wallet = Self::require_wallet(server)?;
        let wallet_clone = Arc::clone(&wallet);
        let privkey_value = request.wif;
        let account = Self::await_wallet_future(Box::pin(async move {
            wallet_clone.import_wif(&privkey_value).await
        }))?;
        Self::save_wallet(&wallet)?;
        Ok(Self::account_to_json(account.as_ref()))
    }

    pub(super) fn list_address(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "listaddress")?;
        let wallet = Self::require_wallet(server)?;
        let mut entries = Vec::new();
        for account in wallet.accounts() {
            entries.push(Self::account_to_json(account.as_ref()));
        }
        Ok(Value::Array(entries))
    }

    pub(super) fn open_wallet(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let request = OpenWalletRequest::parse(params)?;
        if !Path::new(&request.path).exists() {
            return Err(RpcException::from(RpcError::wallet_not_found()));
        }
        let system = server.system();
        let settings = system.settings();
        let wallet = Nep6Wallet::from_file(&request.path, &request.password, settings);
        let wallet = match wallet {
            Ok(wallet) => wallet,
            Err(WalletError::InvalidPassword) => {
                return Err(RpcException::from(
                    RpcError::wallet_not_supported().with_data("Invalid password."),
                ));
            }
            Err(WalletError::WalletFileNotFound(_)) => {
                return Err(RpcException::from(RpcError::wallet_not_found()));
            }
            Err(WalletError::Io(ref err)) if err.kind() == ErrorKind::NotFound => {
                return Err(RpcException::from(RpcError::wallet_not_found()));
            }
            Err(err) => {
                return Err(RpcException::from(
                    RpcError::wallet_not_supported().with_data(err.to_string()),
                ));
            }
        };
        let wallet_arc: Arc<dyn CoreWallet> = Arc::new(wallet);
        server.set_wallet(Some(wallet_arc));
        Ok(Value::Bool(true))
    }

    fn account_to_json(account: &(impl WalletAccount + ?Sized)) -> Value {
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
}
