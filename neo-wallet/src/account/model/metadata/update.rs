use alloc::vec::Vec;

use neo_base::hash::Hash160;

use crate::{signer::SignerScopes, WalletError};

use super::super::account::Account;

impl Account {
    pub fn update_signer_metadata(
        &mut self,
        mut scopes: SignerScopes,
        allowed_contracts: Vec<Hash160>,
        allowed_groups: Vec<Vec<u8>>,
    ) -> Result<(), WalletError> {
        if scopes.contains(SignerScopes::WITNESS_RULES) {
            return Err(WalletError::InvalidSignerMetadata(
                "witness rules scope is not supported yet",
            ));
        }
        if scopes.is_empty() {
            scopes = SignerScopes::CALLED_BY_ENTRY;
        }
        if !scopes.is_valid() {
            return Err(WalletError::InvalidSignerMetadata(
                "invalid witness scope combination",
            ));
        }
        if scopes.contains(SignerScopes::GLOBAL)
            && (!allowed_contracts.is_empty() || !allowed_groups.is_empty())
        {
            return Err(WalletError::InvalidSignerMetadata(
                "global scope cannot specify allowed contracts or groups",
            ));
        }
        if scopes.contains(SignerScopes::CUSTOM_CONTRACTS) {
            if allowed_contracts.is_empty() {
                return Err(WalletError::InvalidSignerMetadata(
                    "custom contracts scope requires at least one contract",
                ));
            }
        } else if !allowed_contracts.is_empty() {
            return Err(WalletError::InvalidSignerMetadata(
                "custom contracts scope must be specified when providing allowed contracts",
            ));
        }
        if scopes.contains(SignerScopes::CUSTOM_GROUPS) {
            if allowed_groups.is_empty() {
                return Err(WalletError::InvalidSignerMetadata(
                    "custom groups scope requires at least one group",
                ));
            }
        } else if !allowed_groups.is_empty() {
            return Err(WalletError::InvalidSignerMetadata(
                "custom groups scope must be specified when providing allowed groups",
            ));
        }
        for group in &allowed_groups {
            if group.len() != 33 {
                return Err(WalletError::InvalidSignerMetadata(
                    "allowed groups must be 33-byte compressed public keys",
                ));
            }
        }

        self.signer_scopes = scopes;
        self.allowed_contracts = allowed_contracts;
        self.allowed_groups = allowed_groups;
        Ok(())
    }
}
