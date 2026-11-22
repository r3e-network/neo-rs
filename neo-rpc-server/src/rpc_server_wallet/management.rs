use super::*;
impl RpcServerWallet {
    pub(super) fn close_wallet(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        server.set_wallet(None);
        Ok(Value::Bool(true))
    }

    pub(super) fn dump_priv_key(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let address = expect_string_param(params, 0, "dumpprivkey")?;
        let script_hash = Self::parse_script_hash(server, &address)?;
        let wallet = Self::require_wallet(server)?;
        let account = wallet.get_account(&script_hash).ok_or_else(|| {
            RpcException::from(RpcError::unknown_account().with_data(script_hash.to_string()))
        })?;
        if !account.has_key() {
            return Err(RpcException::from(
                RpcError::unknown_account().with_data(format!("{script_hash} is watch-only")),
            ));
        }
        let wif = account
            .export_wif()
            .map_err(|err| internal_error(err.to_string()))?;
        Ok(Value::String(wif))
    }

    pub(super) fn get_new_address(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        let wallet = Self::require_wallet(server)?;
        let key_pair = KeyPair::generate().map_err(|err| internal_error(err.to_string()))?;
        let wallet_clone = Arc::clone(&wallet);
        let key_bytes = Zeroizing::new(*key_pair.private_key());
        let account =
            Self::await_wallet_future(
                async move { wallet_clone.create_account(key_bytes.as_ref()) },
            )?;
        Self::save_wallet(&wallet)?;
        Ok(Value::String(account.address()))
    }

    pub(super) fn import_priv_key(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let privkey = expect_string_param(params, 0, "importprivkey")?;
        KeyPair::from_wif(&privkey).map_err(|err| invalid_params(format!("invalid WIF: {err}")))?;
        let wallet = Self::require_wallet(server)?;
        let wallet_clone = Arc::clone(&wallet);
        let privkey_value = privkey;
        let account =
            Self::await_wallet_future(async move { wallet_clone.import_wif(&privkey_value) })?;
        Self::save_wallet(&wallet)?;
        Ok(Self::account_to_json(&account))
    }

    pub(super) fn open_wallet(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let path = expect_string_param(params, 0, "openwallet")?;
        let password = expect_string_param(params, 1, "openwallet")?;
        // A08: jail wallet paths; use a single not-found error to avoid path/oracle leaks.
        let resolved = Self::resolve_jailed_wallet_path(server, &path)?;
        let system = server.system();
        let settings = Arc::new(system.settings().clone());
        let wallet = Nep6Wallet::open(
            resolved
                .to_str()
                .ok_or_else(|| RpcException::from(RpcError::wallet_not_found()))?,
            &password,
            settings,
        );
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
                // R-10: never echo backend wallet errors (they embed paths and
                // account material); log them under a correlation identifier.
                return Err(Self::wallet_internal_failure(err));
            }
        };
        let wallet_arc: Arc<dyn CoreWallet> = Arc::new(wallet);
        server.set_wallet(Some(wallet_arc));
        Ok(Value::Bool(true))
    }

    /// Resolves `openwallet` paths under `WalletDirectory` (or CWD when empty).
    fn resolve_jailed_wallet_path(server: &RpcServer, path: &str) -> Result<PathBuf, RpcException> {
        let not_found = || RpcException::from(RpcError::wallet_not_found());
        let requested = Path::new(path);
        if requested
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(not_found());
        }

        let jail_root = {
            let configured = server.settings().wallet_directory.trim();
            if configured.is_empty() {
                std::env::current_dir().map_err(|_| not_found())?
            } else {
                PathBuf::from(configured)
            }
        };
        let jail = jail_root
            .canonicalize()
            .unwrap_or_else(|_| jail_root.clone());

        let candidate = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            jail.join(requested)
        };

        let canonical = candidate.canonicalize().map_err(|_| not_found())?;
        if !canonical.starts_with(&jail) {
            return Err(not_found());
        }
        Ok(canonical)
    }

    fn save_wallet(wallet: &Arc<dyn CoreWallet>) -> Result<(), RpcException> {
        let wallet_clone = Arc::clone(wallet);
        Self::await_wallet_future(async move { wallet_clone.save() })
    }
}
