use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;
use num_bigint::BigInt;

use super::PolicyContract;

impl PolicyContract {
    /// C# `NEO.GetCommittee(snapshot)`: decodes NeoToken's `Prefix_Committee`
    /// cache (an Array of `Struct[pubkey, votes]`, C#
    /// `CachedCommittee.ToStackItem`) and returns the public keys sorted ascending
    /// (`OrderBy(p => p)`). Faults when the cache is missing, matching the C#
    /// indexer throw.
    pub(in crate::policy_contract) fn read_neo_committee_sorted(
        &self,
        snapshot: &DataCache,
    ) -> CoreResult<Vec<ECPoint>> {
        let key = crate::NeoToken::committee_key();
        let item = snapshot.get(&key).ok_or_else(|| {
            CoreError::invalid_operation("NeoToken committee cache is not initialized")
        })?;
        let decoded = crate::support::codec::decode_stack_value(
            &item.value_bytes(),
            "committee cache",
        )?;
        let committee = crate::neo_token::CachedCommittee::from_stack_value(decoded)?;
        let mut points = committee
            .into_members()
            .into_iter()
            .map(|(point, _votes)| point)
            .collect::<Vec<_>>();
        points.sort();
        Ok(points)
    }

    /// C# `NativeContract.AssertAlmostFullCommittee`: requires a witness from the
    /// `max(max(1, n - (n - 1) / 2), n - 2)`-of-`n` multisig over the committee
    /// public keys ("signed by maximum of (half committee + 1) and
    /// (committee - 2)") and returns that multisig address. Used by `recoverFund`.
    pub(in crate::policy_contract) fn assert_almost_full_committee(
        &self,
        engine: &ApplicationEngine,
    ) -> CoreResult<UInt160> {
        let snapshot = engine.snapshot_cache();
        let committees = self.read_neo_committee_sorted(&snapshot)?;
        // C# AssertAlmostFullCommittee: m = max(max(1, committee majority), n - 2).
        // The `n - (n - 1) / 2` majority term is single-sourced in NeoToken;
        // `n.max(1)` reproduces the original `max(1, …)` guard without underflow.
        let n = committees.len();
        let m = std::cmp::max(
            crate::NeoToken::committee_threshold(n.max(1)),
            n.saturating_sub(2),
        );
        let script =
            neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
                m,
                &committees,
            )
            .map_err(|e| CoreError::invalid_operation(format!("committee multisig script: {e}")))?;
        let address = UInt160::from_script(&script);
        let authorized = engine.check_witness_hash(&address).map_err(|e| {
            CoreError::invalid_operation(format!("recoverFund committee check: {e}"))
        })?;
        if !authorized {
            return Err(CoreError::invalid_operation(
                "Invalid committee signature. It should be a multisig(max(1,len(committee) - 2))).",
            ));
        }
        Ok(address)
    }

    /// Formats the remaining wait time for `recoverFund`'s rejection message,
    /// mirroring the C# ternary chain in `PolicyContract.RecoverFund`
    /// (`{d}d {h}h {m}m` / `{h}h {m}m {s}s` / `{m}m {s}s` / `{s}s`).
    pub(in crate::policy_contract) fn format_remaining_time(remaining: &BigInt) -> String {
        let zero = BigInt::from(0);
        let days = remaining / 86_400_000;
        let hours = (remaining % 86_400_000) / 3_600_000;
        let minutes = (remaining % 3_600_000) / 60_000;
        let seconds = (remaining % 60_000) / 1_000;
        if days > zero {
            format!("{days}d {hours}h {minutes}m")
        } else if hours > zero {
            format!("{hours}h {minutes}m {seconds}s")
        } else if minutes > zero {
            format!("{minutes}m {seconds}s")
        } else {
            format!("{seconds}s")
        }
    }
}
