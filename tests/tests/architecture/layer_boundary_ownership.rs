//! Exclusive crate ownership and compatibility-facade invariants.

use super::*;

#[test]
fn test_execution_uses_neo_vm_contract_without_a_compatibility_facade() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let execution_root = workspace_root.join("neo-execution");

    let execution_deps = parse_neo_dependencies(&execution_root.join("Cargo.toml"));
    assert!(
        execution_deps
            .iter()
            .any(|dependency| dependency == "neo-vm"),
        "neo-execution must consume the workspace neo-vm semantic authority directly"
    );
    assert!(
        !execution_root.join("src/contracts/contract.rs").exists(),
        "neo-execution must not restore a Contract wrapper or re-export module over neo-vm"
    );
    assert!(
        !execution_root.join("src/runtime/interoperable.rs").exists(),
        "neo-execution must not restore an Interoperable or StackItem facade over neo-vm"
    );
    assert!(
        !execution_root
            .join("src/runtime/notify_event_args.rs")
            .exists(),
        "neo-execution must not restore a NotifyEventArgs facade over neo-payloads"
    );

    let checked_files = [
        "neo-execution/src/lib.rs",
        "neo-execution/src/contracts/mod.rs",
        "neo-execution/src/runtime/mod.rs",
    ];
    let forbidden = [
        "pub mod contract;",
        "mod contract;",
        "pub use contract::Contract",
        "pub use contracts::Contract",
        "pub use neo_vm",
        "pub type Contract =",
        "pub mod interoperable",
        "pub use interoperable::Interoperable",
        "SmartContractStackItem",
        "pub mod notify_event_args",
        "pub use notify_event_args::NotifyEventArgs",
        "pub use neo_primitives::TriggerType",
    ];
    let mut violations = Vec::new();
    for relative_path in checked_files {
        let source = fs::read_to_string(workspace_root.join(relative_path))
            .unwrap_or_else(|error| panic!("failed to read {relative_path}: {error}"));
        for marker in forbidden {
            if source.contains(marker) {
                violations.push(format!("{relative_path} contains `{marker}`"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "neo-vm::Contract must remain the only VM contract type exposed to execution callers:\n{}",
        violations.join("\n")
    );

    let vm_interoperable =
        fs::read_to_string(workspace_root.join("neo-vm/src/runtime/interoperable.rs"))
            .expect("read canonical neo-vm interoperable module");
    assert!(
        !vm_interoperable.contains("SmartContractStackItem"),
        "neo-vm callers must use StackItem directly instead of a duplicate type alias"
    );
}

#[test]
fn test_call_flags_have_one_primitive_owner() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let manifest_root = workspace_root.join("neo-manifest");

    assert!(
        !manifest_root.join("src/protocol/call_flags.rs").exists(),
        "CallFlags belongs exclusively to neo-primitives"
    );

    let library =
        fs::read_to_string(manifest_root.join("src/lib.rs")).expect("read neo-manifest crate root");
    for forbidden in [
        "pub mod call_flags",
        "pub use call_flags::CallFlags",
        "pub use neo_primitives::CallFlags",
    ] {
        assert!(
            !library.contains(forbidden),
            "neo-manifest must not restore CallFlags facade `{forbidden}`"
        );
    }

    for crate_directory in [
        "benches-package",
        "neo-blockchain",
        "neo-execution",
        "neo-manifest",
        "neo-native-contracts",
        "neo-node",
        "neo-oracle-service",
        "neo-payloads",
        "neo-rpc",
        "neo-vm",
    ] {
        let dependencies =
            parse_neo_dependencies(&workspace_root.join(crate_directory).join("Cargo.toml"));
        assert!(
            dependencies
                .iter()
                .any(|dependency| dependency == "neo-primitives"),
            "CallFlags consumer `{crate_directory}` must depend directly on neo-primitives"
        );
    }

    for relative_path in [
        "neo-manifest/src/nef/method_token.rs",
        "neo-execution/src/application_engine/mod.rs",
        "neo-execution/src/tests/application_engine/shadow.rs",
        "neo-rpc/src/server/session/execution.rs",
    ] {
        let source = fs::read_to_string(workspace_root.join(relative_path))
            .unwrap_or_else(|error| panic!("failed to read {relative_path}: {error}"));
        let compact = source.split_whitespace().collect::<String>();
        let grouped_imports_call_flags = |owner: &str| {
            let prefix = format!("{owner}::{{");
            compact.split(&prefix).skip(1).any(|tail| {
                tail.split_once('}').is_some_and(|(items, _)| {
                    items
                        .split(',')
                        .any(|item| item == "CallFlags" || item.starts_with("CallFlagsas"))
                })
            })
        };
        assert!(
            source.contains("neo_primitives::CallFlags")
                || grouped_imports_call_flags("neo_primitives"),
            "CallFlags consumer `{relative_path}` must import the canonical primitive owner"
        );
        assert!(
            !source.contains("neo_manifest::CallFlags")
                && !source.contains("neo_manifest::call_flags")
                && !grouped_imports_call_flags("neo_manifest"),
            "CallFlags consumer `{relative_path}` must not restore the removed manifest facade"
        );
    }
}

#[test]
fn test_network_does_not_reexport_foundation_primitives() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let network_root = workspace_root.join("neo-network");
    let library =
        fs::read_to_string(network_root.join("src/lib.rs")).expect("read neo-network crate root");
    let protocol = fs::read_to_string(network_root.join("src/proto/mod.rs"))
        .expect("read neo-network protocol module");

    let primitive_types = [
        "ContainsTransactionType",
        "InvalidWitnessScopeError",
        "InventoryType",
        "NodeCapabilityType",
        "OracleResponseCode",
        "TransactionAttributeType",
        "TransactionRemovalReason",
        "VerifyResult",
        "WitnessConditionType",
        "WitnessRuleAction",
        "WitnessScope",
    ];
    let mut violations = Vec::new();
    for primitive_type in primitive_types {
        if library.contains(primitive_type) {
            violations.push(format!(
                "neo-network/src/lib.rs re-exports `{primitive_type}`"
            ));
        }
        if protocol.contains(primitive_type) {
            violations.push(format!(
                "neo-network/src/proto/mod.rs wraps `{primitive_type}`"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "shared protocol values belong to neo-primitives, while neo-network owns only network-specific protocol and service types:\n{}",
        violations.join("\n")
    );

    for removed in ["src/proto/error.rs", "src/proto/inventory_type.rs"] {
        assert!(
            !network_root.join(removed).exists(),
            "obsolete network compatibility module must stay removed: {removed}"
        );
    }
    assert!(
        !library.contains("P2PError") && !library.contains("P2PResult"),
        "neo-network must expose its canonical NetworkError vocabulary only"
    );
}

#[test]
fn test_p2p_message_command_is_owned_exclusively_by_network() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let primitives_root = workspace_root.join("neo-primitives");
    let network_command = workspace_root.join("neo-network/src/proto/message_command.rs");

    for removed in [
        "src/macros/p2p_message_command.rs",
        "src/errors/network_error.rs",
        "src/tests/errors/network_error.rs",
    ] {
        assert!(
            !primitives_root.join(removed).exists(),
            "network-specific primitive must stay out of neo-primitives: {removed}"
        );
    }

    let mut pending = vec![primitives_root.join("src")];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        {
            let path = entry
                .expect("primitive source entry should be readable")
                .path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            for forbidden in [
                "MessageCommand",
                "p2p_message_command",
                "network_error",
                "P2pError",
                "P2pResult",
                "pub use network_error",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "foundation source `{}` must not own P2P marker `{forbidden}`",
                    path.display()
                );
            }
        }
    }

    let source = fs::read_to_string(&network_command)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", network_command.display()));
    for required in [
        "pub MessageCommand",
        "pub struct MessageCommandParseError",
        "impl FromStr for MessageCommand",
        "impl serde::Serialize for MessageCommand",
        "impl<'de> serde::Deserialize<'de> for MessageCommand",
        "pub const fn allows_compression",
    ] {
        assert!(
            source.contains(required),
            "canonical network command owner is missing `{required}`"
        );
    }
    for forbidden in [
        "neo_primitives::p2p_message_command!",
        "neo_primitives::__p2p_message_command",
        "neo_primitives::NetworkError",
    ] {
        assert!(
            !source.contains(forbidden),
            "network command owner must not restore primitive facade `{forbidden}`"
        );
    }
}

#[test]
fn test_transaction_admission_is_owned_exclusively_by_mempool() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();

    for removed in [
        "neo-mempool/src/admission/transaction_router.rs",
        "neo-mempool/src/tests/admission/transaction_router.rs",
        "neo-blockchain/src/messages/fill_memory_pool.rs",
        "neo-blockchain/src/handlers/providers/transaction.rs",
        "neo-system/src/composition/tx_admission_provider.rs",
    ] {
        assert!(
            !workspace_root.join(removed).exists(),
            "obsolete transaction-admission owner must stay deleted: {removed}"
        );
    }

    let mempool_root = workspace_root.join("neo-mempool/src");
    let memory_pool = fs::read_to_string(mempool_root.join("pool/memory_pool.rs"))
        .expect("read canonical memory pool");
    let origin = fs::read_to_string(mempool_root.join("admission/origin.rs"))
        .expect("read transaction origin");
    let outcome = fs::read_to_string(mempool_root.join("admission/outcome.rs"))
        .expect("read transaction admission outcome");
    for required in [
        "pub fn add_transaction<B, L>",
        "validate_state_independent(",
        "ledger_provider.contains_transaction",
        "ledger_provider.contains_conflict_hash",
        "let mut guard = self.inner.write()",
        "guard.insert_validated(",
    ] {
        assert!(
            memory_pool.contains(required),
            "canonical mempool admission is missing `{required}`"
        );
    }
    assert!(origin.contains("pub enum TransactionOrigin"));
    assert!(origin.contains("External") && origin.contains("Local") && origin.contains("Private"));
    assert!(outcome.contains("pub enum TransactionAdmissionOutcome"));
    assert!(
        outcome.contains("Accepted") && outcome.contains("Rejected") && outcome.contains("Error")
    );

    let handler =
        fs::read_to_string(workspace_root.join("neo-blockchain/src/handlers/transactions.rs"))
            .expect("read blockchain transaction handler");
    assert!(handler.contains("TransactionAdmissionLedger::new"));
    assert!(handler.contains("self.mempool") && handler.contains(".add_transaction(origin"));
    for forbidden in [
        "contains_transaction(",
        "contains_conflict_hash(",
        "verify_state_independent(",
        "verify_state_dependent(",
        "PolicyContract::new()",
    ] {
        assert!(
            !handler.contains(forbidden),
            "blockchain service duplicated mempool policy `{forbidden}`"
        );
    }

    let ledger_context =
        fs::read_to_string(workspace_root.join("neo-blockchain/src/ledger/ledger_context.rs"))
            .expect("read ledger context");
    for forbidden in [
        "transactions_by_hash",
        "fn insert_transaction",
        "fn get_transaction",
    ] {
        assert!(
            !ledger_context.contains(forbidden),
            "LedgerContext must not become a second unconfirmed transaction owner: `{forbidden}`"
        );
    }

    let mut violations = Vec::new();
    for crate_name in [
        "neo-blockchain",
        "neo-system",
        "neo-rpc",
        "neo-node",
        "neo-oracle-service",
    ] {
        let source_root = workspace_root.join(crate_name).join("src");
        let mut pending = vec![source_root.clone()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(&directory)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
            {
                let path = entry.expect("source entry should be readable").path();
                if path.is_dir() {
                    let relative = path
                        .strip_prefix(&source_root)
                        .expect("source-relative path");
                    if !relative
                        .components()
                        .any(|part| part.as_os_str() == "tests")
                    {
                        pending.push(path);
                    }
                    continue;
                }
                if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                    continue;
                }
                let source = fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
                for forbidden in [
                    "pub fn try_add(",
                    "try_add_cached",
                    "PreverifyCompleted",
                    "try_enqueue_preverify",
                    "TxRouterHandle",
                    "FillMemoryPool",
                    "TransactionRouter",
                ] {
                    if source.contains(forbidden) {
                        violations.push(format!("{} contains `{forbidden}`", path.display()));
                    }
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "only neo-mempool may own transaction admission:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_serialization_does_not_reexport_storage_providers() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let serialization_root = workspace_root.join("neo-serialization");

    let dependencies = parse_neo_dependencies(&serialization_root.join("Cargo.toml"));
    assert!(
        !dependencies
            .iter()
            .any(|dependency| dependency == "neo-storage"),
        "neo-serialization owns codecs only; storage providers belong exclusively to neo-storage"
    );
    assert!(
        !serialization_root.join("src/providers").exists(),
        "neo-serialization must not restore a storage-provider re-export facade"
    );

    let library = fs::read_to_string(serialization_root.join("src/lib.rs"))
        .expect("read neo-serialization crate root");
    for forbidden in [
        "pub mod providers",
        "MemorySnapshot",
        "MemoryStore",
        "MemoryStoreProvider",
    ] {
        assert!(
            !library.contains(forbidden),
            "neo-serialization crate root must not expose storage symbol `{forbidden}`"
        );
    }
}

#[test]
fn test_payloads_own_protocol_data_without_storage_or_service_facades() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let payloads_root = workspace_root.join("neo-payloads");

    let dependencies = parse_neo_dependencies(&payloads_root.join("Cargo.toml"));
    assert!(
        !dependencies
            .iter()
            .any(|dependency| dependency == "neo-storage"),
        "neo-payloads must not read storage; stateful verification belongs to domain/node services"
    );
    for removed in [
        "src/execution/event_handlers.rs",
        "src/validation/verify_result.rs",
    ] {
        assert!(
            !payloads_root.join(removed).exists(),
            "obsolete payload facade must stay removed: {removed}"
        );
    }

    let mut pending = vec![payloads_root.join("src")];
    let mut violations = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        {
            let path = entry
                .expect("payload source entry should be readable")
                .path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            for forbidden in [
                "neo_storage",
                "DataCache",
                "CommittedHandler",
                "CommittingHandler",
                "FinalizedHandler",
                "WalletChangedHandler",
                "pub use neo_primitives::VerifyResult",
            ] {
                if source.contains(forbidden) {
                    violations.push(format!("{} contains `{forbidden}`", path.display()));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "payload code must contain only protocol data and storage-independent mechanics:\n{}",
        violations.join("\n")
    );

    let runtime_lifecycle = workspace_root.join("neo-runtime/src/service/lifecycle.rs");
    assert!(
        runtime_lifecycle.is_file(),
        "neo-runtime must remain the canonical owner of service lifecycle contracts"
    );
}

#[test]
fn test_rpc_server_does_not_depend_on_client_transport() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let rpc_root = workspace_root.join("neo-rpc");
    let manifest = read_toml_manifest(&rpc_root.join("Cargo.toml"));
    let server_features = manifest
        .get("features")
        .and_then(|features| features.get("server"))
        .and_then(toml::Value::as_array)
        .expect("neo-rpc should declare a server feature");
    assert!(
        !server_features
            .iter()
            .any(|feature| feature.as_str() == Some("client")),
        "neo-rpc server must not enable the outbound client transport"
    );

    let mut pending = vec![rpc_root.join("src/server")];
    let mut violations = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        {
            let path = entry.expect("RPC source entry should be readable").path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            for forbidden in ["crate::client", "neo_rpc::client"] {
                if source.contains(forbidden) {
                    violations.push(format!("{} contains `{forbidden}`", path.display()));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "RPC server code must consume transport-neutral types and codecs:\n{}",
        violations.join("\n")
    );

    for removed in [
        "src/errors/error.rs",
        "src/client/models/contracts/rpc_contract_state.rs",
        "src/client/models/contracts/rpc_method_token.rs",
        "src/client/models/contracts/rpc_nef_file.rs",
        "src/client/models/ledger/rpc_raw_mem_pool.rs",
        "src/client/models/network/rpc_get_peers.rs",
        "src/client/models/network/rpc_peers.rs",
    ] {
        assert!(
            !rpc_root.join(removed).exists(),
            "obsolete RPC ownership path must stay removed: {removed}"
        );
    }
    for required in [
        "src/client/errors/client.rs",
        "src/protocol/address.rs",
        "src/types/contract_state.rs",
        "src/types/method_token.rs",
        "src/types/nef_file.rs",
        "src/types/peers.rs",
        "src/types/raw_mempool.rs",
    ] {
        assert!(
            rpc_root.join(required).is_file(),
            "canonical RPC ownership path is missing: {required}"
        );
    }

    let crate_root = fs::read_to_string(rpc_root.join("src/lib.rs"))
        .expect("neo-rpc crate root should be readable");
    assert!(
        !crate_root.contains("RpcClientError"),
        "RpcClientError belongs under neo_rpc::client, not a crate-root compatibility facade"
    );
}

#[test]
fn test_storage_backend_selection_is_closed_and_statically_dispatched() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let storage_root = workspace_root.join("neo-storage");

    assert!(
        !storage_root
            .join("src/persistence/traits/store_provider.rs")
            .exists(),
        "neo-storage must not restore the unused StoreProvider extension trait"
    );

    let factory = fs::read_to_string(storage_root.join("src/persistence/traits/store_factory.rs"))
        .expect("read neo-storage store factory");
    for required in [
        "enum StoreBackend",
        "Self::Memory => MemoryStoreProvider::new()",
        "Self::Mdbx => MdbxStoreProvider::new",
    ] {
        assert!(
            factory.contains(required),
            "closed storage backend selection is missing `{required}`"
        );
    }
    for forbidden in [
        "pub enum StoreBackend",
        "pub trait StoreProvider",
        "P: StoreProvider",
    ] {
        assert!(
            !factory.contains(forbidden),
            "storage factory must not restore open provider facade `{forbidden}`"
        );
    }
}

#[test]
fn test_composition_surfaces_retain_one_chain_spec() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();

    for crate_name in ["neo-network", "neo-system", "neo-rpc"] {
        let dependencies =
            parse_neo_dependencies(&workspace_root.join(crate_name).join("Cargo.toml"));
        assert!(
            dependencies
                .iter()
                .any(|dependency| dependency == "neo-config"),
            "{crate_name} must obtain NeoChainSpec and ChainSpecProvider from neo-config"
        );
    }

    let checked_surfaces = [
        (
            "neo-network/src/service/local_node.rs",
            &["chain_spec: Arc<NeoChainSpec>"][..],
        ),
        (
            "neo-system/src/composition/node.rs",
            &[
                "chain_spec: Arc<NeoChainSpec>",
                "impl<P, S> ChainSpecProvider for Node<P, S>",
            ][..],
        ),
        (
            "neo-system/src/composition/builder.rs",
            &["chain_spec: Arc<NeoChainSpec>"][..],
        ),
        (
            "neo-system/src/composition/core.rs",
            &["chain_spec: Arc<NeoChainSpec>"][..],
        ),
        (
            "neo-system/src/composition/system_context.rs",
            &["chain_spec: Arc<NeoChainSpec>"][..],
        ),
        (
            "neo-rpc/src/server/node_context.rs",
            &[
                "chain_spec: Arc<NeoChainSpec>",
                "impl<P, S> ChainSpecProvider for NodeContext<P, S>",
            ][..],
        ),
    ];
    let duplicate_settings_fields = [
        "\n    settings: Arc<ProtocolSettings>",
        "\n    protocol_settings: Arc<ProtocolSettings>",
        "\n    settings: ProtocolSettings",
        "\n    protocol_settings: ProtocolSettings",
    ];
    let mut violations = Vec::new();

    for (relative_path, required) in checked_surfaces {
        let source = fs::read_to_string(workspace_root.join(relative_path))
            .unwrap_or_else(|error| panic!("failed to read {relative_path}: {error}"));
        for marker in required {
            if !source.contains(marker) {
                violations.push(format!("{relative_path} is missing `{marker}`"));
            }
        }
        for marker in duplicate_settings_fields {
            if source.contains(marker) {
                violations.push(format!(
                    "{relative_path} contains duplicate field `{marker}`"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "node composition must retain one immutable NeoChainSpec and derive protocol settings from it:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_genesis_builder_uses_the_authoritative_chain_spec() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let genesis_path = workspace_root.join("neo-blockchain/src/pipeline/native_persist/genesis.rs");
    let source = fs::read_to_string(&genesis_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", genesis_path.display()));

    for required in [
        "pub fn genesis_block(chain_spec: &NeoChainSpec)",
        "let genesis = chain_spec.genesis();",
        "header.set_timestamp(genesis.timestamp);",
        "header.set_nonce(genesis.nonce);",
        "let settings = chain_spec.protocol_settings();",
    ] {
        assert!(
            source.contains(required),
            "canonical genesis construction is missing `{required}`"
        );
    }
    for forbidden in [
        "pub fn genesis_block(settings: &ProtocolSettings)",
        "header.set_timestamp(GENESIS_TIMESTAMP_MS)",
        "header.set_nonce(GENESIS_NONCE)",
    ] {
        assert!(
            !source.contains(forbidden),
            "canonical genesis construction must not restore `{forbidden}`"
        );
    }
}
