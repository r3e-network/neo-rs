//! Exact FlamingoSwapFactory pair-key specialization candidate.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_primitives::constants::MAINNET_MAGIC;
use neo_primitives::{TriggerType, UInt160};
use neo_vm::{
    ArgumentContract, CandidateAuthority, CandidateContract, CandidateContractError,
    CandidateContractLimits, CandidateContractParts, CandidateId, CandidateIdentity,
    CandidateVersion, ContextDependency, ContractResolutionIdentity, EffectContract,
    ExecutionPlanKey, FaultClass, FaultContract, FaultEffectDisposition, GasAmount,
    GasStepContract, HardforkTableIdentity, HostEffectContract, InstructionCount,
    InvocationEligibility, NativeCacheDependency, PointStateDependency, ProtocolIdentity,
    ProtocolVersion, RangeStateDependency, SlotContract, SlotSource, StackEffectContract,
    StackItem, StackItemConstraint, StackItemEligibility, StackItemShape, StackItemType,
    StateDependencyContract,
};
use std::str::FromStr;
use std::sync::{Arc, LazyLock};

/// Stable ID for the exact FlamingoSwapFactory offset-391 candidate.
pub const FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID: CandidateId = CandidateId::new(1);
/// Exact implementation version for the Flamingo pair-key candidate.
pub const FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION: CandidateVersion = CandidateVersion::new(1);
/// Internal helper byte offset in the exact deployed script.
pub const FLAMINGO_FACTORY_PAIR_KEY_ENTRY: u32 = 391;

const CONTRACT_ID: i32 = 27;
const UPDATE_COUNTER: u16 = 1;
const NEF_CHECKSUM: u32 = 2_962_741_568;
const FALSE_BRANCH_INSTRUCTIONS: u64 = 18;
const TRUE_BRANCH_INSTRUCTIONS: u64 = 19;
const FALSE_BRANCH_GAS_UNITS: u64 = 24_678;
const TRUE_BRANCH_GAS_UNITS: u64 = 24_680;

const SCRIPT_BASE64: &str = concat!(
    "DAEAi9shQFcAAXhK2ShQygAUs6skBxDbICIGeBCzqiICQErZKFDKABSzq0AQs0A0DkH4J+yMQEH4J+yMQFcBAAwKc3VwZXJB",
    "ZG1pbjQgcGhK2CQDygAUlyYQaErYJAlKygAUKAM6IgNYIgJAVwABeEGb9mfOQZJd6DEiAkBBkl3oMUBBm/ZnzkDKQFcAAQwP",
    "SW52YWxpZCBBZGRyZXNzeDVn////NCsMCUZvcmJpZGRlbjSLQfgn7Iw0F3gMCnN1cGVyQWRtaW40JhHbICICQFcAAniqJhbC",
    "SnnPDAVGYXVsdEGVAW9hENsgOUA5QFcAAnl4QZv2Z85B5j8YhEBB5j8YhEBXAAMMEU5vIGF1dGhvcml6YXRpb24uNRX///80",
    "sXp5eDcAAEA3AABAVwACeAwRSWRlbnRpY2FsIEFkZHJlc3N4eZg0C3l4ND00JCICQFcAA3iqJhnCSnnPSnrPDAVGYXVsdEGV",
    "AW9hENsgOUBXAAF4QZv2Z85Bkl3oMSICQEGSXegxQFcAAngMAQCL2yF5DAEAi9shtSYJWXiLeYsiB1l5i3iLIgJAi0BXAgMM",
    "CUZvcmJpZGRlbjWD/v//Qfgn7Iw1DP///wwPSW52YWxpZCBBZGRyZXNzeDUs/v//JAcQ2yAiCHk1H/7//zXj/v//eAwRSWRl",
    "bnRpY2FsIEFkZHJlc3N4eZg1SP///3l4NXf///9waDVZ////cQwYRXhjaGFuZ2UgQWxyZWFkeSBFeGlzdGVkaQuXJgcR2yAi",
    "BmnKEJc1i/7//3poNCfCSnjPSnnPSnrPDA5DcmVhdGVFeGNoYW5nZUGVAW9hEdsgIgJAVwACeXhBm/ZnzkHmPxiEQEHmPxiE",
    "QFcEAQwJRm9yYmlkZGVuNab9//9B+CfsjDUv/v//eDcBAHAMDE5vdCBEZXBsb3llZGgLmDUU/v//EMQAFQwJZ2V0VG9rZW4w",
    "eEFifVtScRDEABUMCWdldFRva2VuMXhBYn1bUnIMDVRva2VuIEludmFsaWRpC5gkBxDbICIFaguYNcf9//9qaTV3/v//c3hr",
    "NVv////CSmnPSmrPSnjPDA5DcmVhdGVFeGNoYW5nZUGVAW9hEdsgIgJANwEAQEFifVtSQFcCAgwPSW52YWxpZCBBZGRyZXNz",
    "eDWm/P//JAcQ2yAiCHk1mfz//zVd/f//DAlGb3JiaWRkZW41uvz//0H4J+yMNUP9//95eDXz/f//cGg11f3//3FpStgkA8oQ",
    "tyYhaDQkwkp4z0p5zwwOUmVtb3ZlRXhjaGFuZ2VBlQFvYRHbICICQFcAAXjbKEGb9mfOQS9Yxe1AQS9Yxe1A2yhAVwcAWTRs",
    "cBDEAHFoQZwI7ZwmW2hB81S/HXJqEc4LmCZLahHOc2oQzgAUjXRqEM4AFI51xUoLz0oLz0oLz0o0WkoQbNsoStgkCUrKABQo",
    "AzrQShFt2yhK2CQJSsoAFCgDOtBKEmvQdmluzyKhaSICQFcAARJ4QZv2Z85B3zC4miICQEHfMLiaQEGcCO2cQEHzVL8dQI1A",
    "jkBXAAFA2yhK2CQJSsoAFCgDOkDPQM9AVwACedsoeEGb9mfOQeY/GIRAVwACedsoeEGb9mfOQeY/GIRAVwACeXhBm/ZnzkHm",
    "PxiEQEHmPxiEQFcAAXhBm/ZnzkEvWMXtQFYCDBS930CY+HjJ+zPAP882gqYEPNU1hmAMAf/bMGFA",
);

static SCRIPT_BYTES: LazyLock<Arc<[u8]>> = LazyLock::new(|| {
    let bytes = BASE64_STANDARD
        .decode(SCRIPT_BASE64)
        .expect("embedded FlamingoSwapFactory script must be valid base64");
    assert_eq!(bytes.len(), 1_281, "embedded script length changed");
    Arc::from(bytes)
});

/// Fresh pure result and exact ordinary-VM accounting for one eligible call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlamingoPairKeyArtifact {
    result: StackItem,
    lower_first: bool,
    instructions: u64,
    gas_units: u64,
}

impl FlamingoPairKeyArtifact {
    /// Fresh Buffer result (`ff || lower_token || higher_token`).
    #[must_use]
    pub const fn result(&self) -> &StackItem {
        &self.result
    }

    /// Candidate-local branch decision used by instruction and gas contracts.
    #[must_use]
    pub const fn lower_first(&self) -> bool {
        self.lower_first
    }

    /// Exact ordinary instruction count for the selected branch.
    #[must_use]
    pub const fn instructions(&self) -> u64 {
        self.instructions
    }

    /// Exact ordinary opcode fee units for the selected branch.
    #[must_use]
    pub const fn gas_units(&self) -> u64 {
        self.gas_units
    }
}

/// Fail-closed reason why the exact helper cannot use the candidate kernel.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FlamingoPairKeyEligibilityError {
    /// Per-instruction diagnostics or profiling require the ordinary path.
    #[error("Flamingo pair-key specialization does not support diagnostics")]
    DiagnosticsEnabled,
    /// Per-opcode fee bypass requires the ordinary path.
    #[error("Flamingo pair-key specialization does not support fee-whitelisted contexts")]
    FeeWhitelisted,
    /// The helper must receive exactly two normalized arguments.
    #[error("Flamingo pair-key specialization requires two arguments, got {actual}")]
    Arity {
        /// Observed normalized argument count.
        actual: usize,
    },
    /// Only an immutable 20-byte NeoVM ByteString is accepted.
    #[error("Flamingo pair-key argument {index} must be a 20-byte ByteString")]
    Argument {
        /// Zero-based normalized argument index.
        index: usize,
    },
    /// Static field 1 must be the initialized mutable Buffer `[ff]`.
    #[error("Flamingo pair-key static prefix must be Buffer [ff]")]
    StaticPrefix,
}

/// Computes one fresh exact pair key without reading or caching host state.
///
/// `arguments` are normalized in source order (`tokenA`, then `tokenB`). The
/// caller must separately prove remaining gas, instruction, item-size, stack,
/// and effect-journal bounds before applying the returned artifact.
pub fn try_flamingo_pair_key(
    arguments: &[StackItem],
    static_prefix: &StackItem,
    diagnostics_enabled: bool,
    fee_whitelisted: bool,
) -> Result<FlamingoPairKeyArtifact, FlamingoPairKeyEligibilityError> {
    if diagnostics_enabled {
        return Err(FlamingoPairKeyEligibilityError::DiagnosticsEnabled);
    }
    if fee_whitelisted {
        return Err(FlamingoPairKeyEligibilityError::FeeWhitelisted);
    }
    let [token_a, token_b] = arguments else {
        return Err(FlamingoPairKeyEligibilityError::Arity {
            actual: arguments.len(),
        });
    };
    let StackItem::ByteString(token_a) = token_a else {
        return Err(FlamingoPairKeyEligibilityError::Argument { index: 0 });
    };
    let StackItem::ByteString(token_b) = token_b else {
        return Err(FlamingoPairKeyEligibilityError::Argument { index: 1 });
    };
    if token_a.len() != 20 {
        return Err(FlamingoPairKeyEligibilityError::Argument { index: 0 });
    }
    if token_b.len() != 20 {
        return Err(FlamingoPairKeyEligibilityError::Argument { index: 1 });
    }
    let StackItem::Buffer(prefix) = static_prefix else {
        return Err(FlamingoPairKeyEligibilityError::StaticPrefix);
    };
    if !prefix.with_data(|bytes| bytes == [0xFF]) {
        return Err(FlamingoPairKeyEligibilityError::StaticPrefix);
    }

    // The ordinary script appends 00 before Integer conversion, so comparison
    // is unsigned little-endian. Equal-length values can be compared from the
    // most significant byte without allocating BigInts.
    let lower_first = token_a.iter().rev().cmp(token_b.iter().rev()).is_lt();
    let (lower, higher) = if lower_first {
        (token_a.as_slice(), token_b.as_slice())
    } else {
        (token_b.as_slice(), token_a.as_slice())
    };
    let mut result = Vec::with_capacity(41);
    result.push(0xFF);
    result.extend_from_slice(lower);
    result.extend_from_slice(higher);

    Ok(FlamingoPairKeyArtifact {
        result: StackItem::from_buffer(result),
        lower_first,
        instructions: if lower_first {
            TRUE_BRANCH_INSTRUCTIONS
        } else {
            FALSE_BRANCH_INSTRUCTIONS
        },
        gas_units: if lower_first {
            TRUE_BRANCH_GAS_UNITS
        } else {
            FALSE_BRANCH_GAS_UNITS
        },
    })
}

/// Builds the immutable shadow-only declaration for one exact hardfork table.
pub fn flamingo_pair_key_candidate(
    hardforks: HardforkTableIdentity,
) -> Result<CandidateContract, CandidateContractError> {
    let contract_hash = UInt160::from_str("0xca2d20610d7982ebe0bed124ee7e9b2d580a6efc")
        .expect("embedded FlamingoSwapFactory contract hash must be valid");
    let identity = CandidateIdentity::new(
        FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
        FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
        ExecutionPlanKey::new(
            Arc::clone(&SCRIPT_BYTES),
            FLAMINGO_FACTORY_PAIR_KEY_ENTRY,
            ProtocolIdentity::new(MAINNET_MAGIC, ProtocolVersion::NEO_N3_V3_10_1),
            hardforks,
            TriggerType::APPLICATION,
            Some(ContractResolutionIdentity::new(
                contract_hash,
                CONTRACT_ID,
                UPDATE_COUNTER,
                NEF_CHECKSUM,
            )),
        ),
    );
    let token = StackItemEligibility::new(vec![StackItemShape::new(
        StackItemType::ByteString,
        StackItemConstraint::ByteLength { min: 20, max: 20 },
    )?])?;
    let prefix = StackItemEligibility::new(vec![StackItemShape::new(
        StackItemType::Buffer,
        StackItemConstraint::ExactBytes(Arc::from([0xFF])),
    )?])?;
    let result = StackItemEligibility::new(vec![StackItemShape::new(
        StackItemType::Buffer,
        StackItemConstraint::ByteLength { min: 41, max: 41 },
    )?])?;

    CandidateContract::try_new(
        CandidateContractParts {
            identity,
            authority: CandidateAuthority::ShadowOnly,
            eligibility: InvocationEligibility::new(
                vec![
                    ArgumentContract::new(0, token.clone()),
                    ArgumentContract::new(1, token),
                ],
                vec![SlotContract::new(SlotSource::Static, 1, prefix)],
                vec![
                    ContextDependency::GasRemaining,
                    ContextDependency::FeeWhitelist { expected: false },
                    ContextDependency::InternalCallFrame,
                    ContextDependency::Diagnostics,
                ],
            ),
            state: StateDependencyContract::new(
                Vec::<PointStateDependency>::new(),
                Vec::<RangeStateDependency>::new(),
                Vec::<NativeCacheDependency>::new(),
            ),
            instruction_count: InstructionCount::Decision {
                decision: 0,
                when_true: TRUE_BRANCH_INSTRUCTIONS,
                when_false: FALSE_BRANCH_INSTRUCTIONS,
            },
            gas_steps: Arc::from([GasStepContract {
                id: 0,
                amount: GasAmount::Decision {
                    decision: 0,
                    when_true: TRUE_BRANCH_GAS_UNITS,
                    when_false: FALSE_BRANCH_GAS_UNITS,
                },
                exhaustion_fault: 0,
            }]),
            faults: Arc::from([FaultContract {
                id: 0,
                class: FaultClass::OutOfGas,
                effects: FaultEffectDisposition::Discard,
            }]),
            effects: EffectContract::new(
                StackEffectContract::new(2, 3, vec![result]),
                Vec::<HostEffectContract>::new(),
            ),
        },
        CandidateContractLimits::DEFAULT,
    )
}

#[cfg(test)]
#[path = "../tests/specialization/flamingo_factory_pair_key.rs"]
mod tests;
