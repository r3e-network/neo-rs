use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum Role {
    StateValidator = 4,
    Oracle    = 8,
    NeoFSAlphabet = 16,
    P2pNotary = 32,
}
