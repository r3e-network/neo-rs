use super::{
    block::BlockCommands, blockchain::BlockchainCommands, contracts::ContractCommands,
    logger::LoggerCommands, native::NativeCommands, nep17::Nep17Commands, network::NetworkCommands,
    node::NodeCommands, plugins::PluginCommands, tools::ToolCommands, vote::VoteCommands,
    wallet::WalletCommands, CommandResult,
};
use crate::console_service::{
    ArgumentValue, CommandDispatcher, CommandHandler, ConsoleCommandAttribute, ConsoleHelper,
    ParameterDescriptor, ParameterKind, ParseMode,
};
use anyhow::{anyhow, Result};
use neo_core::{network::p2p::payloads::inventory_type::InventoryType, UInt256};
use std::sync::Arc;

/// Command routing infrastructure (`MainService.CommandLine`).
pub struct CommandLine {
    dispatcher: CommandDispatcher,
}

impl CommandLine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        wallet_commands: Arc<WalletCommands>,
        plugin_commands: Arc<PluginCommands>,
        logger_commands: Arc<LoggerCommands>,
        block_commands: Arc<BlockCommands>,
        blockchain_commands: Arc<BlockchainCommands>,
        native_commands: Arc<NativeCommands>,
        node_commands: Arc<NodeCommands>,
        network_commands: Arc<NetworkCommands>,
        nep17_commands: Arc<Nep17Commands>,
        tool_commands: Arc<ToolCommands>,
        contract_commands: Arc<ContractCommands>,
        vote_commands: Arc<VoteCommands>,
    ) -> Self {
        let mut dispatcher = CommandDispatcher::new();
        Self::register_wallet_commands(&mut dispatcher, wallet_commands);
        Self::register_plugin_commands(&mut dispatcher, plugin_commands);
        Self::register_logger_commands(&mut dispatcher, logger_commands);
        Self::register_block_commands(&mut dispatcher, block_commands);
        Self::register_blockchain_commands(&mut dispatcher, Arc::clone(&blockchain_commands));
        Self::register_native_commands(&mut dispatcher, native_commands);
        Self::register_node_commands(&mut dispatcher, node_commands);
        Self::register_network_commands(&mut dispatcher, network_commands);
        Self::register_vote_commands(&mut dispatcher, vote_commands);
        Self::register_help_command(&mut dispatcher);
        Self::register_nep17_commands(&mut dispatcher, nep17_commands);
        Self::register_tool_commands(&mut dispatcher, tool_commands);
        Self::register_contract_commands(&mut dispatcher, contract_commands);
        Self { dispatcher }
    }

    pub fn execute(&self, command_line: &str) -> CommandResult {
        if self.dispatcher.execute(command_line)? {
            Ok(())
        } else {
            Err(anyhow!("unknown command: {}", command_line.trim()))
        }
    }

    /// Runs an interactive shell that reads commands from stdin.
    pub fn run_shell(&self) -> CommandResult {
        loop {
            let line = match ConsoleHelper::read_user_input("neo", false) {
                Ok(line) => line,
                Err(err) => return Err(err),
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if matches!(trimmed.to_ascii_lowercase().as_str(), "exit" | "quit") {
                break;
            }

            if let Err(err) = self.execute(trimmed) {
                ConsoleHelper::error(err.to_string());
            }
        }
        Ok(())
    }

    fn register_wallet_commands(dispatcher: &mut CommandDispatcher, wallet: Arc<WalletCommands>) {
        let open_handler = wallet_open_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("open wallet"),
            vec![
                ParameterDescriptor::new("path", ParameterKind::String),
                ParameterDescriptor::new("password", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
            ],
            ParseMode::Auto,
            open_handler,
        );

        let close_handler = wallet_close_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("close wallet"),
            Vec::new(),
            ParseMode::Sequential,
            close_handler,
        );

        let create_handler = wallet_create_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("create wallet"),
            vec![
                ParameterDescriptor::new("path", ParameterKind::String),
                ParameterDescriptor::new("password", ParameterKind::String),
            ],
            ParseMode::Auto,
            create_handler,
        );

        let upgrade_handler = wallet_upgrade_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("upgrade wallet"),
            vec![ParameterDescriptor::new("path", ParameterKind::String)],
            ParseMode::Sequential,
            upgrade_handler,
        );

        let export_handler = wallet_export_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("export db3wallet"),
            vec![
                ParameterDescriptor::new("source", ParameterKind::String),
                ParameterDescriptor::new("destination", ParameterKind::String),
            ],
            ParseMode::Sequential,
            export_handler,
        );

        let create_address_handler = wallet_create_address_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("create address"),
            vec![ParameterDescriptor::new("count", ParameterKind::Int)
                .with_default(ArgumentValue::Int(1))],
            ParseMode::Auto,
            create_address_handler,
        );

        let list_handler = wallet_list_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("list address"),
            Vec::new(),
            ParseMode::Sequential,
            list_handler,
        );

        let asset_handler = wallet_asset_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("list asset"),
            Vec::new(),
            ParseMode::Sequential,
            asset_handler,
        );

        let key_handler = wallet_key_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("list key"),
            Vec::new(),
            ParseMode::Sequential,
            key_handler,
        );

        let delete_handler = wallet_delete_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("delete address"),
            vec![ParameterDescriptor::new("address", ParameterKind::String)],
            ParseMode::Auto,
            delete_handler,
        );

        let import_key = wallet_import_key_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("import key"),
            vec![ParameterDescriptor::new("input", ParameterKind::String)],
            ParseMode::Auto,
            import_key,
        );

        let import_watch = wallet_import_watch_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("import watchonly"),
            vec![ParameterDescriptor::new("input", ParameterKind::String)],
            ParseMode::Auto,
            import_watch,
        );

        let import_multisig = wallet_import_multisig_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("import multisigaddress"),
            vec![
                ParameterDescriptor::new("m", ParameterKind::Int),
                ParameterDescriptor::new("public_keys", ParameterKind::String),
            ],
            ParseMode::Auto,
            import_multisig,
        );

        let export_handler = wallet_export_key_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("export key"),
            vec![
                ParameterDescriptor::new("path", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("script_hash", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
            ],
            ParseMode::Auto,
            export_handler,
        );

        let change_password = wallet_change_password_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("change password"),
            Vec::new(),
            ParseMode::Sequential,
            change_password,
        );

        let send_handler = wallet_send_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("send"),
            vec![
                ParameterDescriptor::new("asset", ParameterKind::String),
                ParameterDescriptor::new("to", ParameterKind::String),
                ParameterDescriptor::new("amount", ParameterKind::String),
                ParameterDescriptor::new("from", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("data", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("signers", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
            ],
            ParseMode::Auto,
            send_handler,
        );

        let show_gas_handler = wallet_show_gas_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("show gas"),
            Vec::new(),
            ParseMode::Sequential,
            show_gas_handler,
        );

        let sign_handler = wallet_sign_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("sign"),
            vec![ParameterDescriptor::new("context", ParameterKind::String)],
            ParseMode::Sequential,
            sign_handler,
        );

        let cancel_handler = wallet_cancel_handler(wallet);
        dispatcher.register_command(
            ConsoleCommandAttribute::new("cancel"),
            vec![
                ParameterDescriptor::new("txid", ParameterKind::String),
                ParameterDescriptor::new("sender", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("signers", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
            ],
            ParseMode::Auto,
            cancel_handler,
        );
    }

    fn register_plugin_commands(dispatcher: &mut CommandDispatcher, plugins: Arc<PluginCommands>) {
        let list_handler = plugin_list_handler(Arc::clone(&plugins));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("plugins"),
            Vec::new(),
            ParseMode::Sequential,
            list_handler,
        );

        let active_handler = plugin_active_handler(Arc::clone(&plugins));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("plugins active"),
            Vec::new(),
            ParseMode::Sequential,
            active_handler,
        );

        let install_handler = plugin_install_handler(Arc::clone(&plugins));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("install"),
            vec![
                ParameterDescriptor::new("name", ParameterKind::String),
                ParameterDescriptor::new("url", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
            ],
            ParseMode::Auto,
            install_handler,
        );

        let uninstall_handler = plugin_uninstall_handler(Arc::clone(&plugins));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("uninstall"),
            vec![ParameterDescriptor::new("name", ParameterKind::String)],
            ParseMode::Auto,
            uninstall_handler,
        );

        let reinstall_handler = plugin_reinstall_handler(plugins);
        dispatcher.register_command(
            ConsoleCommandAttribute::new("reinstall"),
            vec![
                ParameterDescriptor::new("name", ParameterKind::String),
                ParameterDescriptor::new("url", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
            ],
            ParseMode::Auto,
            reinstall_handler,
        );
    }

    fn register_logger_commands(dispatcher: &mut CommandDispatcher, logger: Arc<LoggerCommands>) {
        let on_handler = console_log_on_handler(Arc::clone(&logger));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("console log on"),
            Vec::new(),
            ParseMode::Sequential,
            on_handler,
        );

        let off_handler = console_log_off_handler(logger);
        dispatcher.register_command(
            ConsoleCommandAttribute::new("console log off"),
            Vec::new(),
            ParseMode::Sequential,
            off_handler,
        );
    }

    fn register_block_commands(dispatcher: &mut CommandDispatcher, blocks: Arc<BlockCommands>) {
        let export_handler = block_export_handler(Arc::clone(&blocks));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("export blocks"),
            vec![
                ParameterDescriptor::new("start", ParameterKind::Int),
                ParameterDescriptor::new("count", ParameterKind::Int)
                    .with_default(ArgumentValue::Int(u32::MAX as i64)),
                ParameterDescriptor::new("path", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
            ],
            ParseMode::Auto,
            export_handler,
        );

        let show_handler = block_show_handler(blocks);
        dispatcher.register_command(
            ConsoleCommandAttribute::new("show block"),
            vec![ParameterDescriptor::new(
                "index_or_hash",
                ParameterKind::String,
            )],
            ParseMode::Auto,
            show_handler,
        );
    }

    fn register_blockchain_commands(
        dispatcher: &mut CommandDispatcher,
        blockchain: Arc<BlockchainCommands>,
    ) {
        let show_tx_handler = show_transaction_handler(Arc::clone(&blockchain));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("show tx"),
            vec![ParameterDescriptor::new("hash", ParameterKind::String)],
            ParseMode::Auto,
            show_tx_handler,
        );

        let show_contract_handler = show_contract_handler(Arc::clone(&blockchain));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("show contract"),
            vec![ParameterDescriptor::new(
                "name_or_hash",
                ParameterKind::String,
            )],
            ParseMode::Auto,
            show_contract_handler,
        );
    }

    fn register_native_commands(dispatcher: &mut CommandDispatcher, native: Arc<NativeCommands>) {
        let list_handler = native_list_handler(native);
        dispatcher.register_command(
            ConsoleCommandAttribute::new("list nativecontract"),
            Vec::new(),
            ParseMode::Sequential,
            list_handler,
        );
    }

    fn register_node_commands(dispatcher: &mut CommandDispatcher, node: Arc<NodeCommands>) {
        let show_pool_handler = show_pool_handler(Arc::clone(&node));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("show pool"),
            vec![ParameterDescriptor::new("verbose", ParameterKind::Bool)
                .with_default(ArgumentValue::Bool(false))],
            ParseMode::Auto,
            show_pool_handler,
        );

        let show_state_handler = show_state_handler(Arc::clone(&node));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("show state"),
            Vec::new(),
            ParseMode::Sequential,
            show_state_handler,
        );
    }

    fn register_network_commands(
        dispatcher: &mut CommandDispatcher,
        network: Arc<NetworkCommands>,
    ) {
        let show_nodes = show_nodes_handler(Arc::clone(&network));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("show node"),
            Vec::new(),
            ParseMode::Sequential,
            show_nodes,
        );

        let relay_handler = relay_handler(Arc::clone(&network));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("relay"),
            vec![ParameterDescriptor::new("context", ParameterKind::String)],
            ParseMode::Auto,
            relay_handler,
        );

        let ping_handler = broadcast_ping_handler(Arc::clone(&network));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("broadcast ping"),
            Vec::new(),
            ParseMode::Sequential,
            ping_handler,
        );

        let getblocks_handler = broadcast_getblocks_handler(Arc::clone(&network));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("broadcast getblocks"),
            vec![ParameterDescriptor::new("hash", ParameterKind::String)],
            ParseMode::Auto,
            getblocks_handler,
        );

        let getheaders_handler = broadcast_getheaders_handler(Arc::clone(&network));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("broadcast getheaders"),
            vec![ParameterDescriptor::new("start", ParameterKind::Int)],
            ParseMode::Auto,
            getheaders_handler,
        );

        let broadcast_inv = broadcast_inv_handler(Arc::clone(&network));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("broadcast inv"),
            vec![
                ParameterDescriptor::new("type", ParameterKind::String),
                ParameterDescriptor::new("hashes", ParameterKind::String),
            ],
            ParseMode::Auto,
            broadcast_inv,
        );

        let broadcast_getdata = broadcast_getdata_handler(Arc::clone(&network));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("broadcast getdata"),
            vec![
                ParameterDescriptor::new("type", ParameterKind::String),
                ParameterDescriptor::new("hashes", ParameterKind::String),
            ],
            ParseMode::Auto,
            broadcast_getdata,
        );

        let broadcast_tx = broadcast_transaction_handler(Arc::clone(&network));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("broadcast transaction"),
            vec![ParameterDescriptor::new("hash", ParameterKind::String)],
            ParseMode::Auto,
            broadcast_tx,
        );

        let broadcast_block = broadcast_block_handler(Arc::clone(&network));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("broadcast block"),
            vec![ParameterDescriptor::new(
                "index_or_hash",
                ParameterKind::String,
            )],
            ParseMode::Auto,
            broadcast_block,
        );

        let broadcast_addr = broadcast_addr_handler(network);
        dispatcher.register_command(
            ConsoleCommandAttribute::new("broadcast addr"),
            vec![
                ParameterDescriptor::new("host", ParameterKind::String),
                ParameterDescriptor::new("port", ParameterKind::Int),
            ],
            ParseMode::Auto,
            broadcast_addr,
        );
    }

    fn register_help_command(dispatcher: &mut CommandDispatcher) {
        let handler = help_handler(dispatcher.list_commands());
        dispatcher.register_command(
            ConsoleCommandAttribute::new("help"),
            Vec::new(),
            ParseMode::Sequential,
            handler,
        );
    }

    fn register_vote_commands(dispatcher: &mut CommandDispatcher, votes: Arc<VoteCommands>) {
        let register_handler = vote_register_candidate_handler(Arc::clone(&votes));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("register candidate"),
            vec![ParameterDescriptor::new("account", ParameterKind::String)],
            ParseMode::Sequential,
            register_handler,
        );

        let unregister_handler = vote_unregister_candidate_handler(Arc::clone(&votes));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("unregister candidate"),
            vec![ParameterDescriptor::new("account", ParameterKind::String)],
            ParseMode::Sequential,
            unregister_handler,
        );

        let vote_handler = vote_vote_handler(Arc::clone(&votes));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("vote"),
            vec![
                ParameterDescriptor::new("account", ParameterKind::String),
                ParameterDescriptor::new("public_key", ParameterKind::String),
            ],
            ParseMode::Sequential,
            vote_handler,
        );

        let unvote_handler = vote_unvote_handler(Arc::clone(&votes));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("unvote"),
            vec![ParameterDescriptor::new("account", ParameterKind::String)],
            ParseMode::Sequential,
            unvote_handler,
        );

        let candidates_handler = vote_get_candidates_handler(Arc::clone(&votes));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("get candidates"),
            Vec::new(),
            ParseMode::Sequential,
            candidates_handler,
        );

        let committee_handler = vote_get_committee_handler(Arc::clone(&votes));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("get committee"),
            Vec::new(),
            ParseMode::Sequential,
            committee_handler,
        );

        let validators_handler = vote_get_next_validators_handler(Arc::clone(&votes));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("get next validators"),
            Vec::new(),
            ParseMode::Sequential,
            validators_handler,
        );

        let account_state_handler = vote_get_account_state_handler(votes);
        dispatcher.register_command(
            ConsoleCommandAttribute::new("get accountstate"),
            vec![ParameterDescriptor::new("account", ParameterKind::String)],
            ParseMode::Sequential,
            account_state_handler,
        );
    }

    fn register_nep17_commands(dispatcher: &mut CommandDispatcher, nep17: Arc<Nep17Commands>) {
        let transfer_handler = nep17_transfer_handler(Arc::clone(&nep17));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("transfer"),
            vec![
                ParameterDescriptor::new("token", ParameterKind::String),
                ParameterDescriptor::new("to", ParameterKind::String),
                ParameterDescriptor::new("amount", ParameterKind::String),
                ParameterDescriptor::new("from", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("data", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("signers", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
            ],
            ParseMode::Auto,
            transfer_handler,
        );

        let balance_handler = nep17_balance_handler(Arc::clone(&nep17));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("balanceOf"),
            vec![
                ParameterDescriptor::new("token", ParameterKind::String),
                ParameterDescriptor::new("account", ParameterKind::String),
            ],
            ParseMode::Auto,
            balance_handler,
        );

        let decimals_handler = nep17_decimals_handler(Arc::clone(&nep17));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("decimals"),
            vec![ParameterDescriptor::new("token", ParameterKind::String)],
            ParseMode::Sequential,
            decimals_handler,
        );

        let name_handler = nep17_name_handler(Arc::clone(&nep17));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("name"),
            vec![ParameterDescriptor::new("token", ParameterKind::String)],
            ParseMode::Sequential,
            name_handler,
        );

        let total_supply_handler = nep17_total_supply_handler(nep17);
        dispatcher.register_command(
            ConsoleCommandAttribute::new("totalSupply"),
            vec![ParameterDescriptor::new("token", ParameterKind::String)],
            ParseMode::Sequential,
            total_supply_handler,
        );
    }

    fn register_tool_commands(dispatcher: &mut CommandDispatcher, tools: Arc<ToolCommands>) {
        let parse_handler = parse_value_handler(Arc::clone(&tools));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("parse"),
            vec![ParameterDescriptor::new("value", ParameterKind::String)],
            ParseMode::Auto,
            parse_handler,
        );

        let parse_script = parse_script_handler(tools);
        dispatcher.register_command(
            ConsoleCommandAttribute::new("parse script"),
            vec![ParameterDescriptor::new("input", ParameterKind::String)],
            ParseMode::Auto,
            parse_script,
        );
    }

    fn register_contract_commands(
        dispatcher: &mut CommandDispatcher,
        contracts: Arc<ContractCommands>,
    ) {
        let deploy_handler = contract_deploy_handler(Arc::clone(&contracts));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("deploy"),
            vec![
                ParameterDescriptor::new("nef", ParameterKind::String),
                ParameterDescriptor::new("manifest", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("data", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
            ],
            ParseMode::Auto,
            deploy_handler,
        );

        let update_handler = contract_update_handler(Arc::clone(&contracts));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("update"),
            vec![
                ParameterDescriptor::new("hash", ParameterKind::String),
                ParameterDescriptor::new("nef", ParameterKind::String),
                ParameterDescriptor::new("manifest", ParameterKind::String),
                ParameterDescriptor::new("sender", ParameterKind::String),
                ParameterDescriptor::new("signers", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("data", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
            ],
            ParseMode::Auto,
            update_handler,
        );

        let invoke_handler = contract_invoke_handler(Arc::clone(&contracts));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("invoke"),
            vec![
                ParameterDescriptor::new("hash", ParameterKind::String),
                ParameterDescriptor::new("operation", ParameterKind::String),
                ParameterDescriptor::new("params", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("sender", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("signers", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("gas", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
            ],
            ParseMode::Auto,
            invoke_handler,
        );

        let test_invoke_handler = contract_test_invoke_handler(Arc::clone(&contracts));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("testinvoke"),
            vec![
                ParameterDescriptor::new("hash", ParameterKind::String),
                ParameterDescriptor::new("operation", ParameterKind::String),
                ParameterDescriptor::new("params", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("gas", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
            ],
            ParseMode::Auto,
            test_invoke_handler,
        );

        let invoke_abi_handler = contract_invoke_abi_handler(Arc::clone(&contracts));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("invokeabi"),
            vec![
                ParameterDescriptor::new("hash", ParameterKind::String),
                ParameterDescriptor::new("operation", ParameterKind::String),
                ParameterDescriptor::new("args", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("sender", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("signers", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
                ParameterDescriptor::new("gas", ParameterKind::String)
                    .with_default(ArgumentValue::String(String::new())),
            ],
            ParseMode::Auto,
            invoke_abi_handler,
        );
    }
}

fn wallet_open_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("open wallet requires <path> [password]"));
        }
        let path = expect_string(&args[0], "path")?;
        let password = match args.get(1) {
            Some(ArgumentValue::String(text)) if !text.is_empty() => text.clone(),
            Some(ArgumentValue::String(_)) => {
                ConsoleHelper::info(["Cancelled"]);
                return Ok(());
            }
            _ => {
                let prompt = ConsoleHelper::read_user_input("password", true)?;
                if prompt.is_empty() {
                    ConsoleHelper::info(["Cancelled"]);
                    return Ok(());
                }
                prompt
            }
        };
        wallet.open_wallet(path, &password)?;
        Ok(())
    })
}

fn wallet_create_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 2 {
            return Err(anyhow!("create wallet requires <path> <password>"));
        }
        let path = expect_string(&args[0], "path")?;
        let password = expect_string(&args[1], "password")?;
        wallet.create_wallet(path, &password)?;
        Ok(())
    })
}

fn wallet_upgrade_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("upgrade wallet requires <path>"));
        }
        let path = expect_string(&args[0], "path")?;
        wallet.upgrade_wallet(&path)
    })
}

fn wallet_export_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 2 {
            return Err(anyhow!(
                "export db3wallet requires <source-db3-path> <destination-nep6-json>"
            ));
        }
        let source = expect_string(&args[0], "source")?;
        let destination = expect_string(&args[1], "destination")?;
        wallet.export_db3_wallet(&source, &destination)
    })
}

fn wallet_create_address_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        let count = if args.is_empty() {
            1
        } else {
            let value = expect_int(&args[0], "count")?;
            if value <= 0 {
                return Err(anyhow!("count must be greater than zero"));
            }
            u16::try_from(value).map_err(|_| anyhow!("count is too large"))?
        };
        wallet.create_addresses(count)?;
        Ok(())
    })
}

fn wallet_list_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| {
        wallet.list_addresses()?;
        Ok(())
    })
}

fn wallet_asset_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| {
        wallet.list_assets()?;
        Ok(())
    })
}

fn wallet_key_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| {
        wallet.list_keys()?;
        Ok(())
    })
}

fn wallet_send_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 3 {
            return Err(anyhow!("send requires <asset> <to> <amount>"));
        }
        let asset = expect_string(&args[0], "asset")?;
        let to = expect_string(&args[1], "to")?;
        let amount = expect_string(&args[2], "amount")?;
        let from = args.get(3).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        let data = args.get(4).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        let signers = args.get(5).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });

        let signer_accounts = signers
            .map(|text| {
                text.split(',')
                    .map(|entry| entry.trim().to_string())
                    .filter(|entry| !entry.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        wallet.send(
            &asset,
            &to,
            &amount,
            from.as_deref(),
            data.as_deref(),
            signer_accounts,
        )
    })
}

fn parse_script_handler(tools: Arc<ToolCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("parse script requires <base64 or path>"));
        }
        let input = expect_string(&args[0], "input")?;
        tools.analyze_script(&input)
    })
}

fn parse_value_handler(tools: Arc<ToolCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("parse requires <value>"));
        }
        let value = expect_string(&args[0], "value")?;
        tools.parse_value(&value)
    })
}

fn contract_deploy_handler(contracts: Arc<ContractCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("deploy requires <nef> [manifest] [data]"));
        }
        let nef = expect_string(&args[0], "nef")?;
        let manifest = args.get(1).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        let data = args.get(2).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        contracts.deploy(&nef, manifest.as_deref(), data.as_deref())
    })
}

fn contract_update_handler(contracts: Arc<ContractCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 4 {
            return Err(anyhow!(
                "update requires <script-hash> <nef> <manifest> <sender> [signers] [data]"
            ));
        }
        let hash = expect_string(&args[0], "hash")?;
        let nef = expect_string(&args[1], "nef")?;
        let manifest = expect_string(&args[2], "manifest")?;
        let sender = expect_string(&args[3], "sender")?;
        let signers = args.get(4).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(
                text.split(',')
                    .map(|entry| entry.trim().to_string())
                    .filter(|entry| !entry.is_empty())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        });
        let data = args.get(5).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        contracts.update(
            &hash,
            &nef,
            &manifest,
            &sender,
            signers.unwrap_or_default(),
            data.as_deref(),
        )
    })
}

fn contract_invoke_handler(contracts: Arc<ContractCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 2 {
            return Err(anyhow!(
                "invoke requires <script-hash> <operation> [params] [sender] [signers] [maxgas]"
            ));
        }
        let hash = expect_string(&args[0], "hash")?;
        let operation = expect_string(&args[1], "operation")?;
        let params = args.get(2).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        let sender = args.get(3).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        let signers = args.get(4).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(
                text.split(',')
                    .map(|entry| entry.trim().to_string())
                    .filter(|entry| !entry.is_empty())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        });
        let max_gas = args.get(5).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        contracts.invoke(
            &hash,
            &operation,
            params.as_deref(),
            sender.as_deref(),
            signers.unwrap_or_default(),
            max_gas.as_deref(),
        )
    })
}

fn contract_test_invoke_handler(contracts: Arc<ContractCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 2 {
            return Err(anyhow!(
                "testinvoke requires <script-hash> <operation> [params] [maxgas]"
            ));
        }
        let hash = expect_string(&args[0], "hash")?;
        let operation = expect_string(&args[1], "operation")?;
        let params = args.get(2).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        let max_gas = args.get(3).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        contracts.test_invoke(&hash, &operation, params.as_deref(), max_gas.as_deref())
    })
}

fn contract_invoke_abi_handler(contracts: Arc<ContractCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 2 {
            return Err(anyhow!(
                "invokeabi requires <script-hash> <operation> [args] [sender] [signers] [maxgas]"
            ));
        }
        let hash = expect_string(&args[0], "hash")?;
        let operation = expect_string(&args[1], "operation")?;
        let abi_args = args.get(2).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        let sender = args.get(3).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        let signers = args.get(4).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(
                text.split(',')
                    .map(|entry| entry.trim().to_string())
                    .filter(|entry| !entry.is_empty())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        });
        let max_gas = args.get(5).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        contracts.invoke_abi(
            &hash,
            &operation,
            abi_args.as_deref(),
            sender.as_deref(),
            signers.unwrap_or_default(),
            max_gas.as_deref(),
        )
    })
}

fn plugin_list_handler(plugins: Arc<PluginCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| plugins.list_plugins())
}

fn plugin_active_handler(plugins: Arc<PluginCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| plugins.list_loaded_plugins())
}

fn plugin_install_handler(plugins: Arc<PluginCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("install requires <name> [url]"));
        }
        let name = expect_string(&args[0], "name")?;
        let url = if args.len() > 1 {
            Some(expect_string(&args[1], "url")?)
        } else {
            None
        };
        let override_url = url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        plugins.install_plugin(&name, override_url)
    })
}

fn plugin_uninstall_handler(plugins: Arc<PluginCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("uninstall requires <name>"));
        }
        let name = expect_string(&args[0], "name")?;
        plugins.uninstall_plugin(&name)
    })
}

fn plugin_reinstall_handler(plugins: Arc<PluginCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("reinstall requires <name> [url]"));
        }
        let name = expect_string(&args[0], "name")?;
        let url = if args.len() > 1 {
            Some(expect_string(&args[1], "url")?)
        } else {
            None
        };
        let override_url = url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        plugins.reinstall_plugin(&name, override_url)
    })
}

fn block_export_handler(blocks: Arc<BlockCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 2 {
            return Err(anyhow!("export blocks requires <start> <count> [path]"));
        }
        let start = expect_u32(&args[0], "start")?;
        let count = expect_u32(&args[1], "count")?;
        let path = match args.get(2) {
            Some(ArgumentValue::String(value)) if !value.trim().is_empty() => Some(value.clone()),
            _ => None,
        };

        let output = path.unwrap_or_else(|| format!("chain.{start}.acc"));
        blocks.export_blocks(start, count, output)
    })
}

fn block_show_handler(blocks: Arc<BlockCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("show block requires <index_or_hash>"));
        }
        let value = expect_string(&args[0], "index_or_hash")?;
        blocks.show_block(&value)
    })
}

fn show_transaction_handler(blockchain: Arc<BlockchainCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("show tx requires <hash>"));
        }
        let hash = expect_string(&args[0], "hash")?;
        blockchain.show_transaction(&hash)
    })
}

fn show_state_handler(node: Arc<NodeCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| node.show_state())
}

fn show_contract_handler(blockchain: Arc<BlockchainCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("show contract requires <name_or_hash>"));
        }
        let value = expect_string(&args[0], "name_or_hash")?;
        blockchain.show_contract(&value)
    })
}

fn native_list_handler(native: Arc<NativeCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| native.list_native_contracts())
}

fn show_pool_handler(node: Arc<NodeCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        let verbose = if args.is_empty() {
            false
        } else {
            match &args[0] {
                ArgumentValue::Bool(value) => *value,
                other => return Err(anyhow!("verbose expects bool, got {:?}", other)),
            }
        };
        node.show_pool(verbose)
    })
}

fn console_log_on_handler(logger: Arc<LoggerCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| logger.console_log_on())
}

fn console_log_off_handler(logger: Arc<LoggerCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| logger.console_log_off())
}

fn show_nodes_handler(network: Arc<NetworkCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| network.show_nodes())
}

fn relay_handler(network: Arc<NetworkCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("relay requires <context_json_or_path>"));
        }
        let payload = expect_string(&args[0], "context")?;
        network.relay(&payload)
    })
}

fn broadcast_ping_handler(network: Arc<NetworkCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| network.broadcast_ping())
}

fn broadcast_getblocks_handler(network: Arc<NetworkCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("broadcast getblocks requires <hash>"));
        }
        let hash = expect_string(&args[0], "hash")?;
        let hash = hash
            .parse()
            .map_err(|_| anyhow!("hash must be a UInt256 hex string"))?;
        network.broadcast_getblocks(&hash)
    })
}

fn broadcast_getheaders_handler(network: Arc<NetworkCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("broadcast getheaders requires <start>"));
        }
        let start = expect_u32(&args[0], "start")?;
        network.broadcast_getheaders(start)
    })
}

fn parse_inventory_type(text: &str) -> Result<InventoryType> {
    match text.trim().to_ascii_lowercase().as_str() {
        "block" => Ok(InventoryType::Block),
        "tx" | "transaction" => Ok(InventoryType::Transaction),
        "extensible" => Ok(InventoryType::Extensible),
        other => Err(anyhow!("unsupported inventory type '{}'", other)),
    }
}

fn parse_hashes(text: &str) -> Result<Vec<UInt256>> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().map_err(|_| anyhow!("invalid UInt256 '{}'", s)))
        .collect()
}

fn broadcast_inv_handler(network: Arc<NetworkCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 2 {
            return Err(anyhow!("broadcast inv requires <type> <hashes>"));
        }
        let inv_type_text = expect_string(&args[0], "type")?;
        let hashes_text = expect_string(&args[1], "hashes")?;
        let inv_type = parse_inventory_type(&inv_type_text)?;
        let hashes = parse_hashes(&hashes_text)?;
        network.broadcast_inv(inv_type, hashes)
    })
}

fn broadcast_getdata_handler(network: Arc<NetworkCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 2 {
            return Err(anyhow!("broadcast getdata requires <type> <hashes>"));
        }
        let inv_type_text = expect_string(&args[0], "type")?;
        let hashes_text = expect_string(&args[1], "hashes")?;
        let inv_type = parse_inventory_type(&inv_type_text)?;
        let hashes = parse_hashes(&hashes_text)?;
        network.broadcast_getdata(inv_type, hashes)
    })
}

fn broadcast_transaction_handler(network: Arc<NetworkCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("broadcast transaction requires <hash>"));
        }
        let hash = expect_string(&args[0], "hash")?;
        let parsed = hash
            .parse()
            .map_err(|_| anyhow!("hash must be a UInt256 hex string"))?;
        network.broadcast_transaction(&parsed)
    })
}

fn broadcast_block_handler(network: Arc<NetworkCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("broadcast block requires <index_or_hash>"));
        }
        let value = expect_string(&args[0], "index_or_hash")?;
        network.broadcast_block(&value)
    })
}

fn broadcast_addr_handler(network: Arc<NetworkCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 2 {
            return Err(anyhow!("broadcast addr requires <host> <port>"));
        }
        let host = expect_string(&args[0], "host")?;
        let port = expect_u32(&args[1], "port")?;
        let port = u16::try_from(port).map_err(|_| anyhow!("port out of range"))?;
        network.broadcast_addr_host(&host, port)
    })
}

fn wallet_delete_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("delete address requires <address>"));
        }
        let address = expect_string(&args[0], "address")?;
        wallet.delete_address(&address)?;
        Ok(())
    })
}

fn wallet_import_key_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("import key requires <wif_or_file>"));
        }
        let value = expect_string(&args[0], "wif_or_file")?;
        wallet.import_key(&value)?;
        Ok(())
    })
}

fn wallet_import_watch_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("import watchonly requires <address_or_file>"));
        }
        let value = expect_string(&args[0], "address_or_file")?;
        wallet.import_watch_only(&value)?;
        Ok(())
    })
}

fn wallet_import_multisig_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 2 {
            return Err(anyhow!("import multisigaddress requires <m> <public_keys>"));
        }
        let m = expect_u32(&args[0], "m")?;
        let keys_raw = expect_string(&args[1], "public_keys")?;
        let keys: Vec<String> = keys_raw
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| entry.to_string())
            .collect();
        wallet.import_multisig(m as u16, keys)?;
        Ok(())
    })
}

fn wallet_export_key_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        let path = args.first().and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        let script_hash = args.get(1).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        let password = ConsoleHelper::read_user_input("password", true)?;
        wallet.export_keys(script_hash.as_deref(), path.as_deref(), &password)
    })
}

fn wallet_change_password_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| wallet.change_password())
}

fn wallet_show_gas_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| wallet.show_gas())
}

fn wallet_sign_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("sign requires <context_json_or_path>"));
        }
        let payload = expect_string(&args[0], "context")?;
        wallet.sign_context(&payload)
    })
}

fn wallet_cancel_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("cancel requires <txid> [sender] [signers]"));
        }
        let txid = expect_string(&args[0], "txid")?;
        let sender = args.get(1).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        let signers = args.get(2).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });

        let signer_accounts = signers
            .map(|text| {
                text.split(',')
                    .map(|entry| entry.trim().to_string())
                    .filter(|entry| !entry.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        wallet.cancel(&txid, sender.as_deref(), signer_accounts)
    })
}

fn nep17_transfer_handler(nep17: Arc<Nep17Commands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 3 {
            return Err(anyhow!("transfer requires <token> <to> <amount>"));
        }
        let token = expect_string(&args[0], "token")?;
        let to = expect_string(&args[1], "to")?;
        let amount = expect_string(&args[2], "amount")?;
        let from = args.get(3).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        let data = args.get(4).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });
        let signers = args.get(5).and_then(|value| match value {
            ArgumentValue::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        });

        let signer_accounts = signers
            .map(|text| {
                text.split(',')
                    .map(|entry| entry.trim().to_string())
                    .filter(|entry| !entry.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        nep17.transfer(
            &token,
            &to,
            &amount,
            from.as_deref(),
            data.as_deref(),
            signer_accounts,
        )
    })
}

fn nep17_balance_handler(nep17: Arc<Nep17Commands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 2 {
            return Err(anyhow!("balanceOf requires <token> <account>"));
        }
        let token = expect_string(&args[0], "token")?;
        let account = expect_string(&args[1], "account")?;
        nep17.balance_of(&token, &account)
    })
}

fn nep17_name_handler(nep17: Arc<Nep17Commands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("name requires <token>"));
        }
        let token = expect_string(&args[0], "token")?;
        nep17.name(&token)
    })
}

fn nep17_decimals_handler(nep17: Arc<Nep17Commands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("decimals requires <token>"));
        }
        let token = expect_string(&args[0], "token")?;
        nep17.decimals(&token)
    })
}

fn nep17_total_supply_handler(nep17: Arc<Nep17Commands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("totalSupply requires <token>"));
        }
        let token = expect_string(&args[0], "token")?;
        nep17.total_supply(&token)
    })
}

fn vote_register_candidate_handler(votes: Arc<VoteCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("register candidate requires <account>"));
        }
        let account = expect_string(&args[0], "account")?;
        votes.register_candidate(&account)
    })
}

fn vote_unregister_candidate_handler(votes: Arc<VoteCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("unregister candidate requires <account>"));
        }
        let account = expect_string(&args[0], "account")?;
        votes.unregister_candidate(&account)
    })
}

fn vote_vote_handler(votes: Arc<VoteCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 2 {
            return Err(anyhow!("vote requires <account> <public_key>"));
        }
        let account = expect_string(&args[0], "account")?;
        let public_key = expect_string(&args[1], "public_key")?;
        votes.vote(&account, &public_key)
    })
}

fn vote_unvote_handler(votes: Arc<VoteCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("unvote requires <account>"));
        }
        let account = expect_string(&args[0], "account")?;
        votes.unvote(&account)
    })
}

fn vote_get_candidates_handler(votes: Arc<VoteCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| votes.get_candidates())
}

fn vote_get_committee_handler(votes: Arc<VoteCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| votes.get_committee())
}

fn vote_get_next_validators_handler(votes: Arc<VoteCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| votes.get_next_validators())
}

fn vote_get_account_state_handler(votes: Arc<VoteCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("get accountstate requires <account>"));
        }
        let account = expect_string(&args[0], "account")?;
        votes.get_account_state(&account)
    })
}

fn wallet_close_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| {
        wallet.close_wallet()?;
        Ok(())
    })
}

fn expect_string(value: &ArgumentValue, name: &str) -> Result<String> {
    match value {
        ArgumentValue::String(text) => Ok(text.clone()),
        other => Err(anyhow!("{name} expects a string argument, got {:?}", other)),
    }
}

fn expect_int(value: &ArgumentValue, name: &str) -> Result<i64> {
    match value {
        ArgumentValue::Int(num) => Ok(*num),
        other => Err(anyhow!(
            "{name} expects an integer argument, got {:?}",
            other
        )),
    }
}

fn expect_u32(value: &ArgumentValue, name: &str) -> Result<u32> {
    let int = expect_int(value, name)?;
    if int < 0 {
        Err(anyhow!("{name} must be non-negative"))
    } else {
        u32::try_from(int).map_err(|_| anyhow!("{name} is out of range"))
    }
}

fn help_handler(commands: Vec<(String, String)>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| {
        ConsoleHelper::info(["Available commands:"]);
        for (command, description) in &commands {
            if description.is_empty() {
                ConsoleHelper::info([" - ", command]);
            } else {
                ConsoleHelper::info([" - ", command, ": ", description]);
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        commands::{
            block::BlockCommands, blockchain::BlockchainCommands, contracts::ContractCommands,
            logger::LoggerCommands, native::NativeCommands, nep17::Nep17Commands,
            network::NetworkCommands, node::NodeCommands, tools::ToolCommands, vote::VoteCommands,
        },
        config::PluginsSection,
    };
    use neo_core::{neo_system::NeoSystem, protocol_settings::ProtocolSettings};
    use std::sync::{atomic::AtomicBool, Arc};

    fn command_line() -> CommandLine {
        let runtime = Box::leak(Box::new(
            tokio::runtime::Runtime::new().expect("tokio runtime"),
        ));
        let _guard = runtime.enter();
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
        let wallet = Arc::new(WalletCommands::new(
            Arc::new(ProtocolSettings::default()),
            Arc::clone(&system),
        ));
        let plugins = Arc::new(PluginCommands::new(&PluginsSection::default()));
        let blocks = Arc::new(BlockCommands::new(Arc::clone(&system)));
        let blockchain = Arc::new(BlockchainCommands::new(Arc::clone(&system)));
        let native = Arc::new(NativeCommands::new(Arc::clone(&system)));
        let node = Arc::new(NodeCommands::new(Arc::clone(&system)));
        let network = Arc::new(NetworkCommands::new(Arc::clone(&system)));
        let console_flag = Arc::new(AtomicBool::new(true));
        let logger = Arc::new(LoggerCommands::new(Some(console_flag)));
        let nep17 = Arc::new(Nep17Commands::new(Arc::clone(&system), Arc::clone(&wallet)));
        let tools = Arc::new(ToolCommands::new(Arc::new(system.settings().clone())));
        let contracts = Arc::new(ContractCommands::new(
            Arc::clone(&system),
            Arc::clone(&wallet),
        ));
        let votes = Arc::new(VoteCommands::new(
            Arc::clone(&system),
            Arc::clone(&wallet),
            Arc::clone(&contracts),
        ));
        CommandLine::new(
            wallet, plugins, logger, blocks, blockchain, native, node, network, nep17, tools,
            contracts, votes,
        )
    }

    #[test]
    fn help_command_succeeds() {
        let cli = command_line();
        assert!(cli.execute("help").is_ok());
    }

    #[test]
    fn unknown_command_errors() {
        let cli = command_line();
        assert!(cli.execute("unknown command").is_err());
    }
}
