//! Remote-ledger proxy policy.
//!
//! The policy catalog decides which RPC methods must be delegated to the
//! upstream ledger endpoint when this process runs without a local ledger.

/// Return whether `method` should be delegated to the upstream RPC endpoint.
///
/// Local node-control methods (`getversion`, `getpeers`, wallet management)
/// stay local because they describe or mutate this process. Ledger, state,
/// relay, invocation, session, indexer, token-tracker, plugin/service
/// inventory, wallet transaction-building, and oracle submission methods are
/// proxied so an RPC-ledger node does not answer from its ephemeral local chain
/// context.
#[must_use]
pub(in crate::server) fn should_proxy_remote_ledger_method(method: &str) -> bool {
    matches!(
        method.to_ascii_lowercase().as_str(),
        "calculatenetworkfee"
            | "canceltransaction"
            | "findstates"
            | "findstorage"
            | "getcandidates"
            | "getcommittee"
            | "getaddressnotifications"
            | "getaddresstransactions"
            | "getapplicationlog"
            | "getbestblockhash"
            | "getblock"
            | "getblockcount"
            | "getblockhash"
            | "getblockheader"
            | "getblockheadercount"
            | "getblockindex"
            | "getblockindexes"
            | "getblocknotifications"
            | "getblocksysfee"
            | "getblocktransactions"
            | "getcontractnotifications"
            | "getcontractstate"
            | "getcontracttransactions"
            | "getindexerstatus"
            | "getnativecontracts"
            | "getnep11balances"
            | "getnep11properties"
            | "getnep11transfers"
            | "getnep17balances"
            | "getnep17transfers"
            | "getnextblockvalidators"
            | "getproof"
            | "getrawmempool"
            | "getrawtransaction"
            | "getstate"
            | "getstateheight"
            | "getstateroot"
            | "getstorage"
            | "gettransactionindex"
            | "gettransactionheight"
            | "gettransactionnotifications"
            | "getunclaimedgas"
            | "getwalletbalance"
            | "getwalletunclaimedgas"
            | "invokecontractverify"
            | "invokefunction"
            | "invokescript"
            | "listplugins"
            | "listservices"
            | "sendfrom"
            | "sendmany"
            | "sendrawtransaction"
            | "sendtoaddress"
            | "submitblock"
            | "submitoracleresponse"
            | "terminatesession"
            | "traverseiterator"
            | "verifyproof"
    )
}
