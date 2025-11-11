use super::*;
use crate::message::{ChangeViewReason, ConsensusMessage, MessageKind, ViewNumber};
use crate::state::{ConsensusState, SnapshotState};
use crate::validator::{Validator, ValidatorId, ValidatorSet};
use crate::{ConsensusError, QuorumDecision, SignedMessage};
use alloc::format;
use neo_base::{encoding::SliceReader, hash::Hash256, NeoDecode, NeoEncode};
use neo_crypto::{
    ecc256::PrivateKey, ecdsa::SIGNATURE_SIZE, Keypair, Secp256r1Sign, SignatureBytes,
};
use rand::{rngs::StdRng, RngCore, SeedableRng};

mod helpers;
mod quorum;
mod snapshot;
mod validation;

const HEIGHT: u64 = 10;
