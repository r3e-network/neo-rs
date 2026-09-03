use super::NeoToken;
use crate::hardfork::Hardfork;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::manifest::ContractEventDescriptor;
use crate::smart_contract::native::metadata_macros::event_descriptor;
use crate::smart_contract::native::method_macros::neo_native_methods;
use crate::smart_contract::native::{FungibleToken, NativeMethod};

impl NeoToken {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        let mut methods = <Self as FungibleToken>::ft_nep17_methods();
        methods.extend(neo_native_methods![
            safe "unclaimedGas", fee = 1 << 17, flags = [READ_STATES], params = [Hash160, Integer], returns = Integer, names = ["account", "end"];
            safe "getAccountState", fee = 1 << 15, flags = [READ_STATES], params = [Hash160], returns = Array, names = ["account"];
            safe "getCandidates", fee = 1 << 22, flags = [READ_STATES], params = [], returns = Array;
            safe "getAllCandidates", fee = 1 << 22, flags = [READ_STATES], params = [], returns = InteropInterface;
            safe "getCandidateVote", fee = 1 << 15, flags = [READ_STATES], params = [PublicKey], returns = Integer, names = ["pubKey"];
            safe "getCommittee", fee = 1 << 16, flags = [READ_STATES], params = [], returns = Array;
            safe "getCommitteeAddress", fee = 1 << 16, flags = [READ_STATES], params = [], returns = Hash160, active = HfCockatrice;
            safe "getNextBlockValidators", fee = 1 << 16, flags = [READ_STATES], params = [], returns = Array;
            safe "getGasPerBlock", fee = 1 << 15, flags = [READ_STATES], params = [], returns = Integer;
            safe "getRegisterPrice", fee = 1 << 15, flags = [READ_STATES], params = [], returns = Integer;
            unsafe "onNEP17Payment", fee = 0, flags = [STATES, ALLOW_NOTIFY], params = [Hash160, Integer, Any], returns = Void, active = HfEchidna, names = ["from", "amount", "data"];
            unsafe "registerCandidate", fee = 0, flags = [STATES], params = [PublicKey], returns = Boolean, deprecated = HfEchidna, names = ["pubkey"];
            unsafe "registerCandidate", fee = 0, flags = [STATES, ALLOW_NOTIFY], params = [PublicKey], returns = Boolean, active = HfEchidna, names = ["pubkey"];
            unsafe "unregisterCandidate", fee = 1 << 16, flags = [STATES], params = [PublicKey], returns = Boolean, deprecated = HfEchidna, names = ["pubkey"];
            unsafe "unregisterCandidate", fee = 1 << 16, flags = [STATES, ALLOW_NOTIFY], params = [PublicKey], returns = Boolean, active = HfEchidna, names = ["pubkey"];
            unsafe "vote", fee = 1 << 16, flags = [STATES], params = [Hash160, PublicKey], returns = Boolean, deprecated = HfEchidna, names = ["account", "voteTo"];
            unsafe "vote", fee = 1 << 16, flags = [STATES, ALLOW_NOTIFY], params = [Hash160, PublicKey], returns = Boolean, active = HfEchidna, names = ["account", "voteTo"];
            unsafe "setGasPerBlock", fee = 1 << 15, flags = [STATES], params = [Integer], returns = Void, names = ["gasPerBlock"];
            unsafe "setRegisterPrice", fee = 1 << 15, flags = [STATES], params = [Integer], returns = Void, names = ["registerPrice"];
        ]);
        methods
    }

    pub(super) fn supported_standards_metadata(
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Vec<String> {
        if settings.is_hardfork_enabled(Hardfork::HfEchidna, block_height) {
            vec!["NEP-17".to_string(), "NEP-27".to_string()]
        } else {
            vec!["NEP-17".to_string()]
        }
    }

    pub(super) fn event_descriptors(
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        let mut events = vec![
            event_descriptor!(
                "Transfer",
                ["from" => Hash160, "to" => Hash160, "amount" => Integer]
            ),
            event_descriptor!(
                "CandidateStateChanged",
                ["pubkey" => PublicKey, "registered" => Boolean, "votes" => Integer]
            ),
            event_descriptor!(
                "Vote",
                ["account" => Hash160, "from" => PublicKey, "to" => PublicKey, "amount" => Integer]
            ),
        ];

        if settings.is_hardfork_enabled(Hardfork::HfCockatrice, block_height) {
            events.push(event_descriptor!(
                "CommitteeChanged",
                ["old" => Array, "new" => Array]
            ));
        }

        events
    }
}
