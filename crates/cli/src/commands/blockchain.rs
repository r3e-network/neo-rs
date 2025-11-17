use super::CommandResult;
use crate::console_service::ConsoleHelper;
use anyhow::{anyhow, bail};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use chrono::{Local, TimeZone};
use hex;
use neo_core::{
    neo_io::Serializable,
    neo_system::NeoSystem,
    network::p2p::payloads::{transaction_attribute::TransactionAttribute, witness::Witness},
    smart_contract::{
        contract_state::ContractState,
        manifest::{
            ContractManifest, ContractPermission, ContractPermissionDescriptor, WildCardContainer,
        },
        native::{ContractManagement, LedgerContract, NativeContract, NativeRegistry},
    },
    UInt160, UInt256,
};
use std::sync::Arc;

/// Blockchain state commands (mirrors `MainService.Blockchain`).
pub struct BlockchainCommands {
    system: Arc<NeoSystem>,
}

impl BlockchainCommands {
    pub fn new(system: Arc<NeoSystem>) -> Self {
        Self { system }
    }

    pub fn show_contract(&self, name_or_hash: &str) -> CommandResult {
        let store = self.system.context().store_cache();
        let registry = NativeRegistry::new();
        let parsed_hash = name_or_hash.parse::<UInt160>().ok();

        let (contract, native) = if let Some(hash) = parsed_hash {
            let contract = ContractManagement::get_contract_from_store_cache(&store, &hash)
                .map_err(|err| anyhow!("failed to read contract {}: {}", name_or_hash, err))?;
            let native = registry.get(&hash);
            (contract, native)
        } else {
            let native = registry.get_by_name(name_or_hash);
            let contract = native
                .as_ref()
                .and_then(|nc| {
                    ContractManagement::get_contract_from_store_cache(&store, &nc.hash()).ok()
                })
                .flatten();
            (contract, native)
        };

        let Some(contract) = contract else {
            let state = if let Some(native) = native {
                if parsed_hash.is_some() {
                    format!("({}) not active yet", native.name())
                } else {
                    "not active yet".to_string()
                }
            } else {
                "doesn't exist".to_string()
            };
            bail!("Contract {} {}.", name_or_hash, state);
        };

        print_contract(&contract, native.as_deref());
        Ok(())
    }

    pub fn show_transaction(&self, hash_text: &str) -> CommandResult {
        let hash: UInt256 = hash_text
            .parse()
            .map_err(|_| anyhow!("Enter a valid transaction hash."))?;

        let store = self.system.context().store_cache();
        let ledger = LedgerContract::new();
        let state = ledger
            .get_transaction_state(&store, &hash)
            .map_err(|err| anyhow!("failed to read transaction state: {}", err))?
            .ok_or_else(|| anyhow!("Transaction {} doesn't exist.", hash_text))?;

        let block_index = state.block_index();
        let block_hash = self
            .system
            .block_hash_at(block_index)
            .ok_or_else(|| anyhow!("block {} is not available", block_index))?;
        let block = self
            .system
            .context()
            .try_get_block(&block_hash)
            .ok_or_else(|| anyhow!("block {} is not available", block_index))?;

        let tx = state.transaction();
        let timestamp = Local
            .timestamp_millis_opt(block.timestamp() as i64)
            .single()
            .unwrap_or_else(|| {
                Local
                    .timestamp_millis_opt(0)
                    .single()
                    .unwrap_or_else(|| Local.timestamp_millis_opt(0).earliest().unwrap())
            });

        ConsoleHelper::info(["", "-------------", "Transaction", "-------------"]);
        ConsoleHelper::info([""]);
        ConsoleHelper::info(["", "        Timestamp: ", &timestamp.to_string()]);
        ConsoleHelper::info(["", "             Hash: ", &tx.hash().to_string()]);
        ConsoleHelper::info(["", "            Nonce: ", &tx.nonce().to_string()]);
        ConsoleHelper::info([
            "",
            "           Sender: ",
            &tx.sender()
                .map(|sender| sender.to_string())
                .unwrap_or_else(|| "<unknown>".to_string()),
        ]);
        ConsoleHelper::info([
            "",
            "  ValidUntilBlock: ",
            &tx.valid_until_block().to_string(),
        ]);
        ConsoleHelper::info([
            "",
            "       FeePerByte: ",
            &format!("{} datoshi", tx.fee_per_byte()),
        ]);
        ConsoleHelper::info([
            "",
            "       NetworkFee: ",
            &format!("{} datoshi", tx.network_fee()),
        ]);
        ConsoleHelper::info([
            "",
            "        SystemFee: ",
            &format!("{} datoshi", tx.system_fee()),
        ]);
        ConsoleHelper::info(["", "           Script: ", &BASE64.encode(tx.script())]);
        ConsoleHelper::info(["", "          Version: ", &tx.version().to_string()]);
        ConsoleHelper::info(["", "       BlockIndex: ", &block.index().to_string()]);
        ConsoleHelper::info(["", "        BlockHash: ", &block_hash.to_string()]);
        ConsoleHelper::info(["", "             Size: ", &format!("{} Byte(s)", tx.size())]);
        ConsoleHelper::info([""]);

        ConsoleHelper::info(["", "-------------", "Signers", "-------------"]);
        ConsoleHelper::info([""]);
        for signer in tx.signers() {
            let rules = if signer.rules.is_empty() {
                "[]".to_string()
            } else {
                let all: Vec<String> = signer
                    .rules
                    .iter()
                    .map(|rule| format!("\"{}\"", rule.to_json()))
                    .collect();
                format!("[{}]", all.join(", "))
            };
            ConsoleHelper::info(["", "             Rules: ", &rules]);
            ConsoleHelper::info(["", "           Account: ", &signer.account.to_string()]);
            ConsoleHelper::info(["", "            Scopes: ", &signer.scopes().to_string()]);

            if signer.allowed_contracts.is_empty() {
                ConsoleHelper::info(["", "  AllowedContracts: ", "[]"]);
            } else {
                let allowed: Vec<String> = signer
                    .allowed_contracts
                    .iter()
                    .map(|c| c.to_string())
                    .collect();
                ConsoleHelper::info([
                    "",
                    "  AllowedContracts: ",
                    &format!("[{}]", allowed.join(", ")),
                ]);
            }

            if signer.allowed_groups.is_empty() {
                ConsoleHelper::info(["", "     AllowedGroups: ", "[]"]);
            } else {
                let groups: Vec<String> = signer
                    .allowed_groups
                    .iter()
                    .map(|g| format!("0x{}", hex::encode(g.as_bytes())))
                    .collect();
                ConsoleHelper::info([
                    "",
                    "     AllowedGroups: ",
                    &format!("[{}]", groups.join(", ")),
                ]);
            }

            ConsoleHelper::info([
                "",
                "              Size: ",
                &format!("{} Byte(s)", signer.size()),
            ]);
            ConsoleHelper::info([""]);
        }

        print_witnesses(tx.witnesses());
        print_attributes(tx.attributes());
        ConsoleHelper::info([""]);
        ConsoleHelper::info(["", "--------------------------------------"]);
        Ok(())
    }
}

fn print_contract(contract: &ContractState, _native: Option<&dyn NativeContract>) {
    let manifest: &ContractManifest = &contract.manifest;

    ConsoleHelper::info(["", "-------------", "Contract", "-------------"]);
    ConsoleHelper::info([""]);
    ConsoleHelper::info(["", "                Name: ", &manifest.name]);
    ConsoleHelper::info(["", "                Hash: ", &contract.hash.to_string()]);
    ConsoleHelper::info(["", "                  Id: ", &contract.id.to_string()]);
    ConsoleHelper::info([
        "",
        "       UpdateCounter: ",
        &contract.update_counter.to_string(),
    ]);
    let supported = if manifest.supported_standards.is_empty() {
        String::new()
    } else {
        manifest.supported_standards.join(" ")
    };
    ConsoleHelper::info(["", "  SupportedStandards: ", &supported]);
    ConsoleHelper::info([
        "",
        "            Checksum: ",
        &contract.nef.checksum.to_string(),
    ]);
    ConsoleHelper::info(["", "            Compiler: ", &contract.nef.compiler]);
    ConsoleHelper::info(["", "          SourceCode: ", &contract.nef.source]);

    let trusts = match &manifest.trusts {
        WildCardContainer::Wildcard => "*".to_string(),
        WildCardContainer::List(list) => {
            let values: Vec<String> = list.iter().map(descriptor_to_string).collect();
            format!("[{}]", values.join(", "))
        }
    };
    ConsoleHelper::info(["", "              Trusts: ", &trusts]);

    if let Some(extra) = &manifest.extra {
        if let Some(obj) = extra.as_object() {
            for (key, value) in obj {
                if let Some(text) = value.as_str() {
                    ConsoleHelper::info(["", &format!("  {:>18}: ", key), text]);
                } else {
                    ConsoleHelper::info(["", &format!("  {:>18}: ", key), &value.to_string()]);
                }
            }
        }
    }
    ConsoleHelper::info([""]);

    ConsoleHelper::info(["", "-------------", "Groups", "-------------"]);
    ConsoleHelper::info([""]);
    if manifest.groups.is_empty() {
        ConsoleHelper::info(["", "  No Group(s)."]);
    } else {
        for group in &manifest.groups {
            let pubkey = group
                .pub_key
                .encode_compressed()
                .map(hex::encode)
                .unwrap_or_else(|_| "<invalid>".to_string());
            ConsoleHelper::info(["", "     PubKey: ", &pubkey]);
            ConsoleHelper::info(["", "  Signature: ", &BASE64.encode(&group.signature)]);
        }
    }
    ConsoleHelper::info([""]);

    ConsoleHelper::info(["", "-------------", "Permissions", "-------------"]);
    ConsoleHelper::info([""]);
    for permission in &manifest.permissions {
        print_permission(permission);
        ConsoleHelper::info([""]);
    }

    ConsoleHelper::info(["", "-------------", "Methods", "-------------"]);
    ConsoleHelper::info([""]);
    for method in &manifest.abi.methods {
        let params: Vec<String> = method
            .parameters
            .iter()
            .map(|p| format!("{:?}", p.param_type))
            .collect();
        ConsoleHelper::info(["", "        Name: ", &method.name]);
        ConsoleHelper::info(["", "        Safe: ", &method.safe.to_string()]);
        ConsoleHelper::info(["", "      Offset: ", &method.offset.to_string()]);
        ConsoleHelper::info(["", "  Parameters: ", &format!("[{}]", params.join(", "))]);
        ConsoleHelper::info(["", "  ReturnType: ", &format!("{:?}", method.return_type)]);
        ConsoleHelper::info([""]);
    }

    ConsoleHelper::info(["", "-------------", "Script", "-------------"]);
    ConsoleHelper::info([""]);
    ConsoleHelper::info(["  ", &BASE64.encode(&contract.nef.script)]);
    ConsoleHelper::info([""]);
    ConsoleHelper::info(["", "--------------------------------"]);
}

fn print_witnesses(witnesses: &[Witness]) {
    ConsoleHelper::info(["", "-------------", "Witnesses", "-------------"]);
    ConsoleHelper::info([""]);
    for witness in witnesses {
        ConsoleHelper::info([
            "",
            "    InvocationScript: ",
            &BASE64.encode(witness.invocation_script()),
        ]);
        ConsoleHelper::info([
            "",
            "  VerificationScript: ",
            &BASE64.encode(witness.verification_script()),
        ]);
        ConsoleHelper::info([
            "",
            "          ScriptHash: ",
            &witness.script_hash().to_string(),
        ]);
        ConsoleHelper::info([
            "",
            "                Size: ",
            &format!("{} Byte(s)", witness.size()),
        ]);
        ConsoleHelper::info([""]);
    }
}

fn print_attributes(attributes: &[TransactionAttribute]) {
    ConsoleHelper::info(["", "-------------", "Attributes", "-------------"]);
    ConsoleHelper::info([""]);
    if attributes.is_empty() {
        ConsoleHelper::info(["", "  No Attribute(s)."]);
        return;
    }

    for attr in attributes {
        match attr {
            TransactionAttribute::Conflicts(c) => {
                ConsoleHelper::info(["", "  Type: ", "Conflicts"]);
                ConsoleHelper::info(["", "  Hash: ", &c.hash.to_string()]);
                ConsoleHelper::info(["", "  Size: ", &format!("{} Byte(s)", c.size())]);
            }
            TransactionAttribute::OracleResponse(o) => {
                ConsoleHelper::info(["", "    Type: ", "OracleResponse"]);
                ConsoleHelper::info(["", "      Id: ", &o.id.to_string()]);
                ConsoleHelper::info(["", "    Code: ", &format!("{:?}", o.code)]);
                ConsoleHelper::info(["", "  Result: ", &BASE64.encode(o.result.as_slice())]);
                ConsoleHelper::info(["", "    Size: ", &format!("{} Byte(s)", o.size())]);
            }
            TransactionAttribute::HighPriority => {
                ConsoleHelper::info(["", "    Type: ", "HighPriority"]);
            }
            TransactionAttribute::NotValidBefore(n) => {
                ConsoleHelper::info(["", "    Type: ", "NotValidBefore"]);
                ConsoleHelper::info(["", "  Height: ", &n.height.to_string()]);
            }
            TransactionAttribute::NotaryAssisted(n) => {
                ConsoleHelper::info(["", "    Type: ", "NotaryAssisted"]);
                ConsoleHelper::info(["", "   NKeys: ", &n.nkeys.to_string()]);
            }
        }
        ConsoleHelper::info([""]);
    }
}

fn print_permission(permission: &ContractPermission) {
    let contract_desc = descriptor_to_string(&permission.contract);
    let methods = match &permission.methods {
        WildCardContainer::Wildcard => "*".to_string(),
        WildCardContainer::List(methods) => {
            format!("[{}]", methods.join(", "))
        }
    };

    ConsoleHelper::info(["", "  Contract: ", &contract_desc]);
    ConsoleHelper::info(["", "   Methods: ", &methods]);
}

fn descriptor_to_string(descriptor: &ContractPermissionDescriptor) -> String {
    match descriptor {
        ContractPermissionDescriptor::Wildcard => "*".to_string(),
        ContractPermissionDescriptor::Hash(hash) => hash.to_string(),
        ContractPermissionDescriptor::Group(group) => hex::encode(group.encoded()),
    }
}
