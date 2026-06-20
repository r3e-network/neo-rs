use neo_config::Hardfork;
use neo_execution::{NativeEvent, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType};
use std::sync::LazyLock;

use super::{NEO_CANDIDATE_STATE_CHANGED_EVENT, NEO_COMMITTEE_CHANGED_EVENT, NEO_VOTE_EVENT};

pub(super) static NEO_TOKEN_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    let int = ContractParameterType::Integer;
    vec![
        // NEP-17 metadata: `[ContractMethod]` with no CpuFee -> fee 0, no flags.
        crate::nep17_symbol_method(),
        crate::nep17_decimals_method(),
        // NEP-17 state reads: CpuFee 1<<15, RequiredCallFlags ReadStates.
        crate::nep17_total_supply_method(read_states),
        crate::nep17_balance_of_method(read_states),
        // NEP-17 transfer(from, to, amount, data) -> Boolean (CpuFee 1<<17,
        // States|AllowCall|AllowNotify; NEO governance runs in OnBalanceChanging).
        crate::nep17_transfer_method(),
        // Governance reads.
        NativeMethod::new("getGasPerBlock", 1 << 15, true, read_states, vec![], int),
        NativeMethod::new("getRegisterPrice", 1 << 15, true, read_states, vec![], int),
        // Committee reads (CpuFee 1<<16 in C#).
        NativeMethod::new(
            "getCommittee",
            1 << 16,
            true,
            read_states,
            vec![],
            ContractParameterType::Array,
        ),
        NativeMethod::new(
            "getCommitteeAddress",
            1 << 16,
            true,
            read_states,
            vec![],
            ContractParameterType::Hash160,
        )
        .with_active_in(Hardfork::HfCockatrice),
        // getAccountState(account) -> NeoAccountState struct (Array) or null.
        NativeMethod::new(
            "getAccountState",
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Array,
        )
        .with_parameter_names(["account"]),
        // unclaimedGas(account, end) -> Integer (CpuFee 1<<17, ReadStates).
        NativeMethod::new(
            "unclaimedGas",
            1 << 17,
            true,
            read_states,
            vec![ContractParameterType::Hash160, int],
            int,
        )
        .with_parameter_names(["account", "end"]),
        // getNextBlockValidators -> ECPoint[] (Array), CpuFee 1<<16 in C#.
        NativeMethod::new(
            "getNextBlockValidators",
            1 << 16,
            true,
            read_states,
            vec![],
            ContractParameterType::Array,
        ),
        // getCandidates -> (ECPoint, BigInteger)[] (Array of Structs), CpuFee 1<<22.
        NativeMethod::new(
            "getCandidates",
            1 << 22,
            true,
            read_states,
            vec![],
            ContractParameterType::Array,
        ),
        // getAllCandidates -> iterator over the registered candidates
        // (InteropInterface), CpuFee 1<<22, ReadStates (NeoToken.cs:537).
        NativeMethod::new(
            "getAllCandidates",
            1 << 22,
            true,
            read_states,
            vec![],
            ContractParameterType::InteropInterface,
        ),
        // getCandidateVote(pubKey) -> votes, or -1 if not a registered
        // candidate. (C# parameter is `ECPoint pubKey` — capital K, unlike
        // registerCandidate's `pubkey`.)
        NativeMethod::new(
            "getCandidateVote",
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::PublicKey],
            int,
        )
        .with_parameter_names(["pubKey"]),
        // Governance writers (committee-gated, States, Void; C# CpuFee 1<<15).
        NativeMethod::new(
            "setRegisterPrice",
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_parameter_names(["registerPrice"]),
        NativeMethod::new(
            "setGasPerBlock",
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_parameter_names(["gasPerBlock"]),
        // Candidate registration (Echidna V1: States|AllowNotify). registerCandidate
        // has no manifest CpuFee (it charges GetRegisterPrice dynamically);
        // unregisterCandidate is CpuFee 1<<16. Both return Boolean.
        // registerCandidate / unregisterCandidate / vote are each a dual
        // registration (C# NeoToken.cs:397/431/456): V0 is genesis-active with
        // RequiredCallFlags=States and DeprecatedIn=HF_Echidna; V1 is
        // ActiveIn=HF_Echidna and adds AllowNotify (the candidate-state-change
        // notification). Exactly one is active at any height.
        NativeMethod::new(
            "registerCandidate",
            0,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::PublicKey],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["pubkey"])
        .with_deprecated_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "registerCandidate",
            0,
            false,
            CallFlags::STATES.bits() | CallFlags::ALLOW_NOTIFY.bits(),
            vec![ContractParameterType::PublicKey],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["pubkey"])
        .with_active_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "unregisterCandidate",
            1 << 16,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::PublicKey],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["pubkey"])
        .with_deprecated_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "unregisterCandidate",
            1 << 16,
            false,
            CallFlags::STATES.bits() | CallFlags::ALLOW_NOTIFY.bits(),
            vec![ContractParameterType::PublicKey],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["pubkey"])
        .with_active_in(Hardfork::HfEchidna),
        // vote(account, voteTo?) -> Boolean. voteTo is a nullable PublicKey
        // (null = clear the vote). V0 States / V1 States|AllowNotify at Echidna.
        NativeMethod::new(
            "vote",
            1 << 16,
            false,
            CallFlags::STATES.bits(),
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::PublicKey,
            ],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["account", "voteTo"])
        .with_deprecated_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "vote",
            1 << 16,
            false,
            CallFlags::STATES.bits() | CallFlags::ALLOW_NOTIFY.bits(),
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::PublicKey,
            ],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["account", "voteTo"])
        .with_active_in(Hardfork::HfEchidna),
        // onNEP17Payment(from, amount, data) -> Void: candidate registration
        // by paying the register price in GAS to the NEO contract. C#
        // `[ContractMethod(Hardfork.HF_Echidna, RequiredCallFlags =
        // CallFlags.States | CallFlags.AllowNotify)]` with no CpuFee
        // (NeoToken.cs:374).
        crate::nep17_payment_method(
            0,
            false,
            CallFlags::STATES.bits() | CallFlags::ALLOW_NOTIFY.bits(),
        )
        .with_active_in(Hardfork::HfEchidna),
    ]
});

/// NEO's `[ContractEvent]` declarations (NeoToken.cs:63-74) plus the inherited
/// `FungibleToken.Transfer` at order 0. C# concatenates the contract
/// constructor's attributes with the base type's and sorts by order, so the
/// manifest lists Transfer, CandidateStateChanged, Vote, CommitteeChanged.
pub(super) static NEO_TOKEN_EVENTS: LazyLock<Vec<NativeEvent>> = LazyLock::new(|| {
    vec![
        crate::fungible_token_transfer_event(),
        NativeEvent::new(
            1,
            NEO_CANDIDATE_STATE_CHANGED_EVENT,
            &[
                ("pubkey", ContractParameterType::PublicKey),
                ("registered", ContractParameterType::Boolean),
                ("votes", ContractParameterType::Integer),
            ],
        ),
        NativeEvent::new(
            2,
            NEO_VOTE_EVENT,
            &[
                ("account", ContractParameterType::Hash160),
                ("from", ContractParameterType::PublicKey),
                ("to", ContractParameterType::PublicKey),
                ("amount", ContractParameterType::Integer),
            ],
        ),
        NativeEvent::new(
            3,
            NEO_COMMITTEE_CHANGED_EVENT,
            &[
                ("old", ContractParameterType::Array),
                ("new", ContractParameterType::Array),
            ],
        )
        .with_active_in(Hardfork::HfCockatrice),
    ]
});
