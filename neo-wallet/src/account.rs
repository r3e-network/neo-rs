use alloc::{string::String, vec, vec::Vec};

use serde_json::{Map as JsonMap, Value};

use core::str::FromStr;
use hex::{decode, encode};
use neo_base::{hash::Hash160, AddressVersion};
use neo_crypto::{
    ecc256::{Keypair, PrivateKey, PublicKey},
    Secp256r1Sign, SignatureBytes,
};

use crate::{
    error::WalletError,
    nep6::{Nep6Account, Nep6Contract, Nep6Parameter, Nep6Scrypt},
    signer::{Signer, SignerScopes},
};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ContractParameterType {
    Signature = 0x00,
    Boolean = 0x01,
    Integer = 0x02,
    Hash160 = 0x03,
    Hash256 = 0x04,
    ByteArray = 0x05,
    PublicKey = 0x06,
    String = 0x07,
    Array = 0x10,
    InteropInterface = 0x11,
    Void = 0xFF,
}

impl From<ContractParameterType> for u8 {
    fn from(value: ContractParameterType) -> Self {
        value as u8
    }
}

impl TryFrom<u8> for ContractParameterType {
    type Error = WalletError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use ContractParameterType::*;
        let ty = match value {
            0x00 => Signature,
            0x01 => Boolean,
            0x02 => Integer,
            0x03 => Hash160,
            0x04 => Hash256,
            0x05 => ByteArray,
            0x06 => PublicKey,
            0x07 => String,
            0x10 => Array,
            0x11 => InteropInterface,
            0xFF => Void,
            _ => return Err(WalletError::InvalidNep6("unknown parameter type")),
        };
        Ok(ty)
    }
}

#[derive(Clone, Debug)]
pub struct ContractParameter {
    name: String,
    parameter_type: ContractParameterType,
}

impl ContractParameter {
    pub fn new(name: impl Into<String>, parameter_type: ContractParameterType) -> Self {
        Self {
            name: name.into(),
            parameter_type,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn parameter_type(&self) -> ContractParameterType {
        self.parameter_type
    }
}

#[derive(Clone, Debug)]
pub struct Contract {
    script: Vec<u8>,
    parameters: Vec<ContractParameter>,
    deployed: bool,
}

impl Contract {
    pub fn new(script: Vec<u8>, parameters: Vec<ContractParameter>, deployed: bool) -> Self {
        Self {
            script,
            parameters,
            deployed,
        }
    }

    pub fn signature(public_key: &PublicKey) -> Self {
        Self {
            script: public_key.signature_redeem_script().to_vec(),
            parameters: vec![ContractParameter::new(
                "signature",
                ContractParameterType::Signature,
            )],
            deployed: false,
        }
    }

    pub fn script(&self) -> &[u8] {
        &self.script
    }

    pub fn parameters(&self) -> &[ContractParameter] {
        &self.parameters
    }

    pub fn deployed(&self) -> bool {
        self.deployed
    }
}

#[derive(Clone, Debug)]
pub struct Account {
    script_hash: Hash160,
    public_key: Option<PublicKey>,
    private_key: Option<PrivateKey>,
    label: Option<String>,
    is_default: bool,
    lock: bool,
    contract: Option<Contract>,
    extra: Option<Value>,
    signer_scopes: SignerScopes,
    allowed_contracts: Vec<Hash160>,
    allowed_groups: Vec<Vec<u8>>,
}

impl Account {
    pub fn from_private_key(private_key: PrivateKey) -> Result<Self, WalletError> {
        let keypair = Keypair::from_private(private_key.clone())
            .map_err(|_| WalletError::Crypto("keypair"))?;
        let script_hash = keypair.public_key.script_hash();
        Ok(Self {
            script_hash,
            public_key: Some(keypair.public_key.clone()),
            private_key: Some(private_key),
            label: None,
            is_default: false,
            lock: false,
            contract: Some(Contract::signature(&keypair.public_key)),
            extra: None,
            signer_scopes: SignerScopes::CALLED_BY_ENTRY,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
        })
    }

    pub fn watch_only(public_key: PublicKey) -> Self {
        let script_hash = public_key.script_hash();
        Self {
            script_hash,
            public_key: Some(public_key.clone()),
            private_key: None,
            label: None,
            is_default: false,
            lock: false,
            contract: Some(Contract::signature(&public_key)),
            extra: None,
            signer_scopes: SignerScopes::CALLED_BY_ENTRY,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
        }
    }

    pub fn watch_only_from_script(script_hash: Hash160, contract: Option<Contract>) -> Self {
        Self {
            script_hash,
            public_key: None,
            private_key: None,
            label: None,
            is_default: false,
            lock: false,
            contract,
            extra: None,
            signer_scopes: SignerScopes::CALLED_BY_ENTRY,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
        }
    }

    pub fn script_hash(&self) -> Hash160 {
        self.script_hash
    }

    pub fn public_key(&self) -> Option<&PublicKey> {
        self.public_key.as_ref()
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = Some(label.into());
    }

    pub fn clear_label(&mut self) {
        self.label = None;
    }

    pub fn is_watch_only(&self) -> bool {
        self.private_key.is_none()
    }

    pub fn is_default(&self) -> bool {
        self.is_default
    }

    pub fn set_default(&mut self, value: bool) {
        self.is_default = value;
    }

    pub fn is_locked(&self) -> bool {
        self.lock
    }

    pub fn set_lock(&mut self, value: bool) {
        self.lock = value;
    }

    pub fn contract(&self) -> Option<&Contract> {
        self.contract.as_ref()
    }

    pub fn set_contract(&mut self, contract: Option<Contract>) {
        self.contract = contract;
    }

    pub fn extra(&self) -> Option<&Value> {
        self.extra.as_ref()
    }

    pub fn set_extra(&mut self, extra: Option<Value>) {
        self.extra = extra;
    }

    pub fn signer_scopes(&self) -> SignerScopes {
        self.signer_scopes
    }

    pub fn set_signer_scopes(&mut self, scopes: SignerScopes) {
        self.signer_scopes = scopes;
    }

    pub fn allowed_contracts(&self) -> &[Hash160] {
        &self.allowed_contracts
    }

    pub fn set_allowed_contracts(&mut self, contracts: Vec<Hash160>) {
        self.allowed_contracts = contracts;
    }

    pub fn allowed_groups(&self) -> &[Vec<u8>] {
        &self.allowed_groups
    }

    pub fn set_allowed_groups(&mut self, groups: Vec<Vec<u8>>) {
        self.allowed_groups = groups;
    }

    pub fn update_signer_metadata(
        &mut self,
        scopes: SignerScopes,
        allowed_contracts: Vec<Hash160>,
        allowed_groups: Vec<Vec<u8>>,
    ) -> Result<(), WalletError> {
        let mut scopes = scopes;
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

    pub fn signer_key(&self) -> Option<&PrivateKey> {
        self.private_key.as_ref()
    }

    pub fn private_key_bytes(&self) -> Option<&[u8]> {
        self.private_key.as_ref().map(|k| k.as_be_bytes())
    }

    pub fn sign(&self, payload: &[u8]) -> Result<SignatureBytes, WalletError> {
        if self.lock {
            return Err(WalletError::AccountLocked);
        }
        let private = self
            .private_key
            .as_ref()
            .ok_or(WalletError::PassphraseRequired)?;
        private
            .secp256r1_sign(payload)
            .map_err(|_| WalletError::Crypto("ecdsa"))
    }

    pub fn to_signer(&self) -> Signer {
        let mut signer = Signer::new(self.script_hash);
        signer.set_scopes(self.signer_scopes);
        signer.set_allowed_contracts(self.allowed_contracts.clone());
        signer.set_allowed_groups(self.allowed_groups.clone());
        signer
    }

    pub fn to_nep6_account(
        &self,
        version: AddressVersion,
        encrypted_key: Option<String>,
    ) -> Result<Nep6Account, WalletError> {
        let contract = self
            .contract
            .as_ref()
            .map(contract_to_nep6);

        let extra = embed_signer_extra(&self.extra, self);

        Ok(Nep6Account {
            address: self.script_hash.to_address(version),
            label: self.label.clone(),
            is_default: self.is_default,
            lock: self.lock,
            key: encrypted_key,
            contract,
            extra,
        })
    }

    pub fn from_nep6_account(
        account: &Nep6Account,
        version: AddressVersion,
        scrypt: Nep6Scrypt,
        password: Option<&str>,
    ) -> Result<Self, WalletError> {
        let script_hash = Hash160::from_address(&account.address, version)
            .map_err(|_| WalletError::InvalidAddress(account.address.clone()))?;

        let private_key = match account.key.as_deref() {
            Some(nep2) => {
                let password = password.ok_or(WalletError::PassphraseRequired)?;
                let scrypt_params = scrypt.into();
                Some(neo_crypto::nep2::decrypt_nep2(
                    nep2,
                    password,
                    version,
                    scrypt_params,
                )?)
            }
            None => None,
        };

        let mut public_key = None;
        if private_key.is_some() {
            public_key = Some(
                Keypair::from_private(private_key.as_ref().unwrap().clone())
                    .map_err(|_| WalletError::Crypto("keypair"))?
                    .public_key,
            );
        }

        let contract = if let Some(contract) = &account.contract {
            Some(contract_from_nep6(contract)?)
        } else if let Some(pk) = public_key.as_ref() {
            Some(Contract::signature(pk))
        } else {
            None
        };

        let (clean_extra, scopes, allowed_contracts, allowed_groups) =
            parse_signer_extra(account.extra.clone())?;

        let mut result = Self {
            script_hash,
            public_key,
            private_key,
            label: account.label.clone(),
            is_default: account.is_default,
            lock: account.lock,
            contract,
            extra: clean_extra,
            signer_scopes: SignerScopes::CALLED_BY_ENTRY,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
        };

        result.signer_scopes = scopes;
        result.allowed_contracts = allowed_contracts;
        result.allowed_groups = allowed_groups;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn account_from_private_key() {
        let private = PrivateKey::new(hex!(
            "c37b8b0c7c0b8c1fe4f602c3f0f2f3536bc3a1ad9ecf15ff86f9fee9b7dd2f75"
        ));
        let account = Account::from_private_key(private.clone()).expect("account");
        assert!(!account.is_watch_only());
        let signature = account.sign(b"neo-wallet").expect("signature");
        assert_eq!(signature.0.len(), 64);
        assert!(account.label().is_none());
        assert!(account.contract().is_some());
    }

    #[test]
    fn watch_only_account() {
        let private = PrivateKey::new([1u8; 32]);
        let account = Account::from_private_key(private.clone()).unwrap();
        let watch = Account::watch_only(account.public_key().unwrap().clone());
        assert!(watch.is_watch_only());
        assert!(watch.sign(&[1, 2, 3]).is_err());
        assert!(watch.contract().is_some());
        let signer = watch.to_signer();
        assert_eq!(signer.scopes(), SignerScopes::CALLED_BY_ENTRY);
    }

    #[test]
    fn update_signer_metadata_requires_contracts_with_scope() {
        let mut account =
            Account::from_private_key(PrivateKey::new([2u8; 32])).expect("account");
        let result = account.update_signer_metadata(
            SignerScopes::CUSTOM_CONTRACTS,
            Vec::new(),
            Vec::new(),
        );
        assert!(matches!(
            result,
            Err(WalletError::InvalidSignerMetadata(_))
        ));

        let contract =
            Hash160::from_slice(&hex!("0102030405060708090a0b0c0d0e0f1011121314")).unwrap();
        account
            .update_signer_metadata(
                SignerScopes::CUSTOM_CONTRACTS,
                vec![contract],
                Vec::new(),
            )
            .expect("metadata applied");
    }

    #[test]
    fn update_signer_metadata_validates_groups() {
        let mut account =
            Account::from_private_key(PrivateKey::new([3u8; 32])).expect("account");
        let invalid_group = vec![0u8; 32];
        let result = account.update_signer_metadata(
            SignerScopes::CUSTOM_GROUPS,
            Vec::new(),
            vec![invalid_group],
        );
        assert!(matches!(
            result,
            Err(WalletError::InvalidSignerMetadata(_))
        ));

        let mut valid_group = vec![0u8; 33];
        valid_group[0] = 0x02;
        account
            .update_signer_metadata(
                SignerScopes::CUSTOM_GROUPS,
                Vec::new(),
                vec![valid_group],
            )
            .expect("metadata applied");
    }

    #[test]
    fn update_signer_metadata_defaults_empty_scope() {
        let mut account =
            Account::from_private_key(PrivateKey::new([4u8; 32])).expect("account");
        account
            .update_signer_metadata(SignerScopes::NONE, Vec::new(), Vec::new())
            .expect("metadata applied");
        assert_eq!(account.signer_scopes(), SignerScopes::CALLED_BY_ENTRY);
    }

    #[test]
    fn account_sign_fails_when_locked() {
        let mut account = Account::from_private_key(PrivateKey::new([5u8; 32])).unwrap();
        account.set_lock(true);
        let err = account.sign(b"payload").unwrap_err();
        assert!(matches!(err, WalletError::AccountLocked));
    }
}

pub(crate) fn contract_to_nep6(contract: &Contract) -> Nep6Contract {
    Nep6Contract {
        script: BASE64.encode(contract.script()),
        parameters: contract
            .parameters()
            .iter()
            .map(|param| Nep6Parameter {
                name: param.name().to_string(),
                type_id: param.parameter_type().into(),
            })
            .collect(),
        deployed: contract.deployed(),
    }
}

pub(crate) fn contract_from_nep6(contract: &Nep6Contract) -> Result<Contract, WalletError> {
    let script = BASE64
        .decode(contract.script.as_bytes())
        .map_err(|_| WalletError::InvalidNep6("invalid contract script encoding"))?;
    let parameters = contract
        .parameters
        .iter()
        .map(|param| {
            Ok(ContractParameter::new(
                param.name.clone(),
                ContractParameterType::try_from(param.type_id)?,
            ))
        })
        .collect::<Result<Vec<_>, WalletError>>()?;
    Ok(Contract::new(script, parameters, contract.deployed))
}

fn embed_signer_extra(extra: &Option<Value>, account: &Account) -> Option<Value> {
    if account.signer_scopes == SignerScopes::CALLED_BY_ENTRY
        && account.allowed_contracts.is_empty()
        && account.allowed_groups.is_empty()
    {
        return extra.clone();
    }

    let signer_value = serialize_signer(account);

    match extra.clone() {
        Some(Value::Object(mut map)) => {
            map.insert("signer".into(), signer_value);
            Some(Value::Object(map))
        }
        Some(other) => {
            let mut map = JsonMap::new();
            map.insert("data".into(), other);
            map.insert("signer".into(), signer_value);
            Some(Value::Object(map))
        }
        None => {
            let mut map = JsonMap::new();
            map.insert("signer".into(), signer_value);
            Some(Value::Object(map))
        }
    }
}

fn serialize_signer(account: &Account) -> Value {
    let mut signer = JsonMap::new();
    signer.insert(
        "scopes".into(),
        Value::String(account.signer_scopes.to_witness_scope_string()),
    );
    if !account.allowed_contracts.is_empty() {
        let contracts = account
            .allowed_contracts
            .iter()
            .map(|hash| Value::String(format!("{hash}")))
            .collect();
        signer.insert("allowedContracts".into(), Value::Array(contracts));
    }
    if !account.allowed_groups.is_empty() {
        let groups = account
            .allowed_groups
            .iter()
            .map(|group| Value::String(format!("0x{}", encode(group))))
            .collect();
        signer.insert("allowedGroups".into(), Value::Array(groups));
    }
    Value::Object(signer)
}

fn parse_signer_extra(
    extra: Option<Value>,
) -> Result<(Option<Value>, SignerScopes, Vec<Hash160>, Vec<Vec<u8>>), WalletError> {
    let mut scopes = SignerScopes::CALLED_BY_ENTRY;
    let mut allowed_contracts = Vec::new();
    let mut allowed_groups = Vec::new();

    if let Some(Value::Object(mut map)) = extra.clone() {
        if let Some(Value::Object(signer)) = map.remove("signer") {
            if let Some(Value::String(scopes_str)) = signer.get("scopes") {
                if let Some(parsed) = SignerScopes::from_witness_scope_string(scopes_str) {
                    if parsed.is_valid() {
                        scopes = parsed;
                    }
                }
            }

            if let Some(Value::Array(contracts)) = signer.get("allowedContracts") {
                for value in contracts {
                    let Some(str_val) = value.as_str() else {
                        return Err(WalletError::InvalidNep6(
                            "allowedContracts entries must be strings",
                        ));
                    };
                    let hash = Hash160::from_str(str_val).map_err(|_| {
                        WalletError::InvalidNep6("invalid script hash in allowedContracts")
                    })?;
                    allowed_contracts.push(hash);
                }
            }

            if let Some(Value::Array(groups)) = signer.get("allowedGroups") {
                for value in groups {
                    let Some(str_val) = value.as_str() else {
                        return Err(WalletError::InvalidNep6(
                            "allowedGroups entries must be strings",
                        ));
                    };
                    let trimmed = str_val.strip_prefix("0x").unwrap_or(str_val);
                    let bytes = decode(trimmed)
                        .map_err(|_| WalletError::InvalidNep6("invalid allowedGroups entry"))?;
                    allowed_groups.push(bytes);
                }
            }
        }
        let remaining = if map.is_empty() {
            None
        } else {
            Some(Value::Object(map))
        };
        return Ok((remaining, scopes, allowed_contracts, allowed_groups));
    }

    Ok((extra, scopes, allowed_contracts, allowed_groups))
}
