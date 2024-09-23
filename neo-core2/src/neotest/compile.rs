use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use neo_go::cli::smartcontract;
use neo_go::compiler;
use neo_go::config;
use neo_go::core::state;
use neo_go::smartcontract::manifest;
use neo_go::smartcontract::nef;
use neo_go::util;
use anyhow::Result;

// Contract contains contract info for deployment.
struct Contract {
    hash: util::Uint160,
    nef: nef::File,
    manifest: manifest::Manifest,
    debug_info: compiler::DebugInfo,
}

// contracts caches the compiled contracts from FS across multiple tests.
static mut CONTRACTS: Option<HashMap<String, Contract>> = None;

// CompileSource compiles a contract from the reader and returns its NEF, manifest and hash.
fn compile_source(t: &mut dyn testing::TB, sender: util::Uint160, src: &mut dyn Read, opts: &compiler::Options) -> Result<Contract> {
    // nef.NewFile() cares about version a lot.
    config::VERSION = "neotest".to_string();

    let (ne, di) = compiler::compile_with_options("contract.go", src, opts)?;
    let m = compiler::create_manifest(&di, opts)?;

    Ok(Contract {
        hash: state::create_contract_hash(sender, ne.checksum, &m.name),
        nef: ne,
        manifest: m,
        debug_info: di,
    })
}

// CompileFile compiles a contract from the file and returns its NEF, manifest and hash.
fn compile_file(t: &mut dyn testing::TB, sender: util::Uint160, src_path: &str, config_path: &str) -> Result<Contract> {
    unsafe {
        if let Some(contracts) = &CONTRACTS {
            if let Some(c) = contracts.get(src_path) {
                return Ok(c.clone());
            }
        }
    }

    // nef.NewFile() cares about version a lot.
    config::VERSION = "neotest".to_string();

    let mut file = File::open(src_path)?;
    let (ne, di) = compiler::compile_with_options(src_path, &mut file, None)?;
    let conf = smartcontract::parse_contract_config(config_path)?;

    let mut o = compiler::Options::default();
    o.name = conf.name;
    o.contract_events = conf.events;
    o.declared_named_types = conf.named_types;
    o.contract_supported_standards = conf.supported_standards;
    o.permissions = conf.permissions.iter().map(|p| manifest::Permission::from(p.clone())).collect();
    o.safe_methods = conf.safe_methods;
    o.overloads = conf.overloads;
    o.source_url = conf.source_url;
    let m = compiler::create_manifest(&di, &o)?;

    let c = Contract {
        hash: state::create_contract_hash(sender, ne.checksum, &m.name),
        nef: ne,
        manifest: m,
        debug_info: di,
    };

    unsafe {
        if let Some(contracts) = &mut CONTRACTS {
            contracts.insert(src_path.to_string(), c.clone());
        } else {
            let mut new_contracts = HashMap::new();
            new_contracts.insert(src_path.to_string(), c.clone());
            CONTRACTS = Some(new_contracts);
        }
    }

    Ok(c)
}
