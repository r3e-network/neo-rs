// Comprehensive test modules converted from C#

pub mod big_decimal_tests;
pub mod smart_contract;
pub mod network;
pub mod ledger;

// Comprehensive test modules
pub mod smart_contract_comprehensive;
pub mod network_comprehensive;
pub mod ledger_comprehensive;
pub mod persistence_comprehensive;
pub mod wallets_comprehensive;
pub mod extensions_comprehensive;

// Plugin test modules
pub mod rpcserver_comprehensive;
pub mod oracleservice_comprehensive;
pub mod dbftplugin_comprehensive;
pub mod stateservice_comprehensive;
pub mod applicationlogs_comprehensive;
pub mod storage_comprehensive;

// Additional converted tests
pub mod persistence {
    pub mod data_cache_tests;
}

pub mod wallets {
    pub mod wallet_tests;
}

pub mod extensions {
    pub mod byte_extensions_tests;
}

pub mod smart_contract {
    pub mod contract_tests;
    pub mod neo_token_tests;
    pub mod gas_token_tests;
}
