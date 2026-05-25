//! Message types for StateService extensible payloads.
//!
//! Matches `Neo.Plugins.StateService.Network.MessageType`.

use neo_primitives::protocol_enum;

protocol_enum! {
    /// StateService message type marker.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub MessageType {
        /// Vote message for state root signatures.
        Vote = 0,
        /// State root message containing the signed root.
        StateRoot = 1,
    }
}

#[cfg(test)]
mod tests {
    use super::MessageType;

    #[test]
    fn message_type_matches_state_service_wire_values() {
        assert_eq!(MessageType::Vote.to_byte(), 0);
        assert_eq!(MessageType::StateRoot.to_byte(), 1);
        assert_eq!(MessageType::from_byte(0), Some(MessageType::Vote));
        assert_eq!(MessageType::from_byte(1), Some(MessageType::StateRoot));
        assert_eq!(MessageType::from_byte(2), None);
    }

    #[test]
    fn message_type_preserves_existing_debug_names() {
        assert_eq!(format!("{:?}", MessageType::Vote), "Vote");
        assert_eq!(format!("{:?}", MessageType::StateRoot), "StateRoot");
    }

    #[test]
    fn message_type_serde_uses_wire_byte() {
        let serialized = serde_json::to_string(&MessageType::StateRoot).unwrap();
        assert_eq!(serialized, "1");

        let deserialized: MessageType = serde_json::from_str("1").unwrap();
        assert_eq!(deserialized, MessageType::StateRoot);

        assert!(serde_json::from_str::<MessageType>("2").is_err());
    }
}
