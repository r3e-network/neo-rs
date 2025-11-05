use alloc::{collections::BTreeMap, vec::Vec};
use core::convert::TryFrom;

use hashbrown::{HashMap, HashSet};
use neo_base::{hash::Hash256, read_varint, write_varint, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use crate::{
    error::ConsensusError,
    message::{ConsensusMessage, MessageKind, SignedMessage, ViewNumber},
    validator::{ValidatorId, ValidatorSet},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuorumDecision {
    Pending,
    Proposal {
        kind: MessageKind,
        proposal: Hash256,
        missing: Vec<ValidatorId>,
    },
    ViewChange {
        new_view: ViewNumber,
        missing: Vec<ValidatorId>,
    },
}

pub struct ConsensusState {
    height: u64,
    view: ViewNumber,
    validators: ValidatorSet,
    records: HashMap<MessageKind, Vec<SignedMessage>>,
    proposal: Option<Hash256>,
    expected: HashMap<MessageKind, Vec<ValidatorId>>,
}

impl ConsensusState {
    pub fn new(height: u64, view: ViewNumber, validators: ValidatorSet) -> Self {
        let mut state = Self {
            height,
            view,
            validators,
            records: HashMap::new(),
            proposal: None,
            expected: HashMap::new(),
        };
        state.seed_prepare_request_expectation();
        state
    }

    pub fn height(&self) -> u64 {
        self.height
    }

    pub fn view(&self) -> ViewNumber {
        self.view
    }

    pub fn validators(&self) -> &ValidatorSet {
        &self.validators
    }

    pub fn proposal(&self) -> Option<Hash256> {
        self.proposal
    }

    pub fn records(&self, kind: MessageKind) -> &[SignedMessage] {
        self.records
            .get(&kind)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn participation_by_kind(&self) -> BTreeMap<MessageKind, Vec<ValidatorId>> {
        let mut map = BTreeMap::new();
        for (kind, messages) in &self.records {
            let validators = messages.iter().map(|m| m.validator).collect();
            map.insert(*kind, validators);
        }
        map
    }

    pub fn tallies(&self) -> BTreeMap<MessageKind, usize> {
        let mut map = BTreeMap::new();
        for (kind, messages) in &self.records {
            map.insert(*kind, messages.len());
        }
        map
    }

    pub fn quorum_threshold(&self) -> usize {
        self.validators.quorum()
    }

    pub fn primary(&self) -> Option<ValidatorId> {
        self.validators.primary_id(self.height, self.view)
    }

    pub fn missing_validators(&self, kind: MessageKind) -> Vec<ValidatorId> {
        let recorded = self.records.get(&kind);
        match kind {
            MessageKind::PrepareRequest => {
                let Some(primary) = self.primary() else {
                    return Vec::new();
                };
                let present = recorded
                    .map(|entries| entries.iter().any(|m| m.validator == primary))
                    .unwrap_or(false);
                if present {
                    Vec::new()
                } else {
                    vec![primary]
                }
            }
            _ => {
                let Some(expected) = self.expected_participants(kind) else {
                    return Vec::new();
                };
                let present = recorded
                    .map(|entries| entries.iter().map(|m| m.validator).collect::<HashSet<_>>())
                    .unwrap_or_default();
                expected
                    .into_iter()
                    .filter(|validator| !present.contains(validator))
                    .collect()
            }
        }
    }

    fn all_validator_ids(&self) -> Vec<ValidatorId> {
        self.validators.iter().map(|v| v.id).collect()
    }

    fn participants_for(&self, kind: MessageKind) -> Vec<ValidatorId> {
        self.records
            .get(&kind)
            .map(|messages| messages.iter().map(|m| m.validator).collect())
            .unwrap_or_default()
    }

    pub fn expected_participants(&self, kind: MessageKind) -> Option<Vec<ValidatorId>> {
        self.expected.get(&kind).cloned()
    }

    pub fn snapshot(&self) -> SnapshotState {
        SnapshotState::from(self)
    }

    pub fn from_snapshot(
        validators: ValidatorSet,
        snapshot: SnapshotState,
    ) -> Result<Self, ConsensusError> {
        let SnapshotState {
            height,
            view,
            proposal,
            participation,
            expected,
        } = snapshot;
        let mut records = HashMap::new();
        let mut proposal = proposal;
        for (kind, messages) in participation {
            let mut seen = HashSet::new();
            for message in &messages {
                if message.kind() != kind {
                    return Err(ConsensusError::InvalidView {
                        expected: view,
                        received: message.view,
                    });
                }
                if validators.get(message.validator).is_none() {
                    return Err(ConsensusError::UnknownValidator(message.validator));
                }
                if message.height != height {
                    return Err(ConsensusError::InvalidHeight {
                        expected: height,
                        received: message.height,
                    });
                }
                match kind {
                    MessageKind::ChangeView => {
                        if message.view != view {
                            return Err(ConsensusError::StaleMessage {
                                kind,
                                current_view: view,
                                message_view: message.view,
                            });
                        }
                    }
                    _ => {
                        if message.view != view {
                            return Err(ConsensusError::InvalidView {
                                expected: view,
                                received: message.view,
                            });
                        }
                    }
                }
                if !seen.insert(message.validator) {
                    return Err(ConsensusError::DuplicateMessage {
                        kind,
                        validator: message.validator,
                    });
                }
                if let ConsensusMessage::PrepareRequest { .. } = message.message {
                    let expected = validators
                        .primary_id(height, view)
                        .ok_or(ConsensusError::NoValidators)?;
                    if message.validator != expected {
                        return Err(ConsensusError::InvalidPrimary {
                            expected,
                            actual: message.validator,
                        });
                    }
                }
                if let Some(hash) = message.message.proposal_hash() {
                    match proposal {
                        Some(current) if current != hash => {
                            return Err(ConsensusError::ProposalMismatch {
                                expected: current,
                                actual: hash,
                            })
                        }
                        None => {
                            proposal = Some(hash);
                        }
                        _ => {}
                    }
                }
            }
            records.insert(kind, messages);
        }

        let mut expected_map = HashMap::new();
        for (kind, validators_list) in expected {
            for validator in &validators_list {
                if validators.get(*validator).is_none() {
                    return Err(ConsensusError::UnknownValidator(*validator));
                }
            }
            expected_map.insert(kind, validators_list);
        }

        let mut state = Self {
            height,
            view,
            validators,
            records,
            proposal,
            expected: expected_map,
        };
        state.seed_prepare_request_expectation();
        Ok(state)
    }

    pub fn register_message(&mut self, message: SignedMessage) -> Result<(), ConsensusError> {
        if message.height != self.height {
            return Err(ConsensusError::InvalidHeight {
                expected: self.height,
                received: message.height,
            });
        }
        if self.validators.get(message.validator).is_none() {
            return Err(ConsensusError::UnknownValidator(message.validator));
        }

        let kind = message.kind();

        match kind {
            MessageKind::ChangeView => {
                if message.view != self.view {
                    return Err(ConsensusError::StaleMessage {
                        kind: MessageKind::ChangeView,
                        current_view: self.view,
                        message_view: message.view,
                    });
                }
            }
            _ if message.view != self.view => {
                return Err(ConsensusError::InvalidView {
                    expected: self.view,
                    received: message.view,
                });
            }
            _ => {}
        }

        {
            let entry = self.records.entry(kind).or_default();
            if entry.iter().any(|m| m.validator == message.validator) {
                return Err(ConsensusError::DuplicateMessage {
                    kind,
                    validator: message.validator,
                });
            }

            if let ConsensusMessage::ChangeView { new_view, .. } = &message.message {
                if *new_view <= self.view {
                    return Err(ConsensusError::StaleView {
                        current: self.view,
                        requested: *new_view,
                    });
                }
                if let Some(existing) = entry.first() {
                    if let ConsensusMessage::ChangeView {
                        new_view: existing_view,
                        ..
                    } = existing.message
                    {
                        if *new_view != existing_view {
                            return Err(ConsensusError::InconsistentView {
                                expected: existing_view,
                                received: *new_view,
                            });
                        }
                    }
                }
            }
        }

        if let ConsensusMessage::PrepareRequest { .. } = message.message {
            let expected = self
                .validators
                .primary_id(self.height, self.view)
                .ok_or(ConsensusError::NoValidators)?;
            if message.validator != expected {
                return Err(ConsensusError::InvalidPrimary {
                    expected,
                    actual: message.validator,
                });
            }
        }

        if !matches!(message.message, ConsensusMessage::PrepareRequest { .. }) {
            if let Some(actual_hash) = message.message.proposal_hash() {
                match self.proposal {
                    Some(expected) => {
                        if expected != actual_hash {
                            return Err(ConsensusError::ProposalMismatch {
                                expected,
                                actual: actual_hash,
                            });
                        }
                    }
                    None => {
                        return Err(ConsensusError::MissingProposal);
                    }
                }
            }
        }

        if let ConsensusMessage::PrepareRequest { proposal_hash, .. } = &message.message {
            match self.proposal {
                None => self.proposal = Some(*proposal_hash),
                Some(existing) if existing != *proposal_hash => {
                    return Err(ConsensusError::ProposalMismatch {
                        expected: existing,
                        actual: *proposal_hash,
                    })
                }
                _ => {}
            }
        }

        if let ConsensusMessage::Commit { .. } = &message.message {
            let responded = self
                .records
                .get(&MessageKind::PrepareResponse)
                .map(|responses| {
                    responses
                        .iter()
                        .any(|entry| entry.validator == message.validator)
                })
                .unwrap_or(false);
            if !responded {
                return Err(ConsensusError::MissingPrepareResponse {
                    validator: message.validator,
                });
            }
        }

        self.records.entry(kind).or_default().push(message);
        self.refresh_expected(kind);
        Ok(())
    }

    pub fn tally(&self, kind: MessageKind) -> usize {
        self.records(kind).len()
    }

    pub fn quorum(&mut self, kind: MessageKind) -> QuorumDecision {
        match kind {
            MessageKind::ChangeView => {
                if let Some(target) = self.change_view_target() {
                    if self.tally(kind) >= self.validators.quorum() {
                        let missing = self.missing_validators(kind);
                        self.expected.remove(&MessageKind::ChangeView);
                        return QuorumDecision::ViewChange {
                            new_view: target,
                            missing,
                        };
                    }
                }
                QuorumDecision::Pending
            }
            _ => {
                if let Some(proposal) = self.proposal {
                    if self.tally(kind) >= self.validators.quorum() {
                        let missing = self.missing_validators(kind);
                        if kind == MessageKind::Commit {
                            self.expected.remove(&MessageKind::Commit);
                        }
                        return QuorumDecision::Proposal {
                            kind,
                            proposal,
                            missing,
                        };
                    }
                }
                QuorumDecision::Pending
            }
        }
    }

    pub fn apply_view_change(&mut self, new_view: ViewNumber) {
        self.view = new_view;
        self.records.clear();
        self.proposal = None;
        self.expected.clear();
        self.seed_prepare_request_expectation();
    }

    pub fn advance_height(&mut self, new_height: u64) -> Result<(), ConsensusError> {
        if new_height <= self.height {
            return Err(ConsensusError::InvalidHeightTransition {
                current: self.height,
                requested: new_height,
            });
        }
        self.height = new_height;
        self.view = ViewNumber::ZERO;
        self.records.clear();
        self.proposal = None;
        self.expected.clear();
        self.seed_prepare_request_expectation();
        Ok(())
    }

    fn change_view_target(&self) -> Option<ViewNumber> {
        self.records
            .get(&MessageKind::ChangeView)
            .and_then(|messages| messages.first())
            .and_then(|msg| match msg.message {
                ConsensusMessage::ChangeView { new_view, .. } => Some(new_view),
                _ => None,
            })
    }

    fn refresh_expected(&mut self, kind: MessageKind) {
        match kind {
            MessageKind::PrepareRequest => {
                if let Some(primary) = self.validators.primary_id(self.height, self.view) {
                    self.expected
                        .insert(MessageKind::PrepareRequest, vec![primary]);
                }
                if !self.expected.contains_key(&MessageKind::PrepareResponse) {
                    self.expected
                        .insert(MessageKind::PrepareResponse, self.all_validator_ids());
                }
            }
            MessageKind::PrepareResponse => {
                let mut responders = self.participants_for(MessageKind::PrepareResponse);
                responders.sort();
                responders.dedup();
                if responders.len() == self.validators.len() {
                    self.expected.remove(&MessageKind::PrepareResponse);
                } else if !self.expected.contains_key(&MessageKind::PrepareResponse) {
                    self.expected
                        .insert(MessageKind::PrepareResponse, self.all_validator_ids());
                }
                if responders.is_empty() {
                    self.expected.remove(&MessageKind::Commit);
                } else {
                    self.expected.insert(MessageKind::Commit, responders);
                }
            }
            MessageKind::Commit => {
                let committers = self.participants_for(MessageKind::Commit);
                let committers: HashSet<_> = committers.into_iter().collect();
                if let Some(entry) = self.expected.get(&MessageKind::Commit) {
                    if entry.iter().all(|validator| committers.contains(validator)) {
                        self.expected.remove(&MessageKind::Commit);
                    }
                }
            }
            MessageKind::ChangeView => {
                if self.records.get(&MessageKind::ChangeView).is_some() {
                    self.expected
                        .insert(MessageKind::ChangeView, self.all_validator_ids());
                } else {
                    self.expected.remove(&MessageKind::ChangeView);
                }
            }
        }
    }

    fn seed_prepare_request_expectation(&mut self) {
        if let Some(primary) = self.validators.primary_id(self.height, self.view) {
            self.expected
                .insert(MessageKind::PrepareRequest, vec![primary]);
        } else {
            self.expected.remove(&MessageKind::PrepareRequest);
        }
    }
}

/// Compact representation suitable for snapshotting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotState {
    pub height: u64,
    pub view: ViewNumber,
    pub proposal: Option<Hash256>,
    pub participation: BTreeMap<MessageKind, Vec<SignedMessage>>,
    pub expected: BTreeMap<MessageKind, Vec<ValidatorId>>,
}

impl From<&ConsensusState> for SnapshotState {
    fn from(state: &ConsensusState) -> Self {
        let mut participation = BTreeMap::new();
        for (kind, messages) in state.records.iter() {
            participation.insert(*kind, messages.clone());
        }

        let mut expected = BTreeMap::new();
        for (kind, validators) in state.expected.iter() {
            expected.insert(*kind, validators.clone());
        }

        Self {
            height: state.height,
            view: state.view,
            proposal: state.proposal,
            participation,
            expected,
        }
    }
}

impl NeoEncode for SnapshotState {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.height.neo_encode(writer);
        self.view.neo_encode(writer);
        match self.proposal {
            Some(hash) => {
                writer.write_u8(1);
                hash.neo_encode(writer);
            }
            None => writer.write_u8(0),
        }
        write_varint(writer, self.participation.len() as u64);
        for (kind, messages) in &self.participation {
            writer.write_u8(kind.as_u8());
            write_varint(writer, messages.len() as u64);
            for message in messages {
                message.neo_encode(writer);
            }
        }
        write_varint(writer, self.expected.len() as u64);
        for (kind, validators) in &self.expected {
            writer.write_u8(kind.as_u8());
            write_varint(writer, validators.len() as u64);
            for validator in validators {
                validator.neo_encode(writer);
            }
        }
    }
}

impl NeoDecode for SnapshotState {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, neo_base::encoding::DecodeError> {
        let height = u64::neo_decode(reader)?;
        let view = ViewNumber::neo_decode(reader)?;
        let proposal = match reader.read_u8()? {
            0 => None,
            _ => Some(Hash256::neo_decode(reader)?),
        };
        let entries = read_varint(reader)? as usize;
        let mut participation = BTreeMap::new();
        for _ in 0..entries {
            let kind = MessageKind::try_from(reader.read_u8()?)?;
            let count = read_varint(reader)? as usize;
            let mut messages = Vec::with_capacity(count);
            for _ in 0..count {
                messages.push(SignedMessage::neo_decode(reader)?);
            }
            participation.insert(kind, messages);
        }

        let mut expected = BTreeMap::new();
        if reader.remaining() > 0 {
            let count = read_varint(reader)? as usize;
            for _ in 0..count {
                let kind = MessageKind::try_from(reader.read_u8()?)?;
                let validators_len = read_varint(reader)? as usize;
                let mut validators = Vec::with_capacity(validators_len);
                for _ in 0..validators_len {
                    validators.push(ValidatorId::neo_decode(reader)?);
                }
                expected.insert(kind, validators);
            }
        }

        Ok(Self {
            height,
            view,
            proposal,
            participation,
            expected,
        })
    }
}
