use super::*;
use crate::ApplicationEngine;
use crate::Diagnostic;
use crate::native_contract::NativeContract;
use crate::native_contract_provider::{NativeContractProvider, NoNativeContractProvider};
use neo_config::{Hardfork, ProtocolSettings};
use neo_error::CoreResult;
use neo_primitives::{ContractParameterType, UInt160};

struct IndexedContract {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl<P> NativeContract<P> for IndexedContract
where
    P: NativeContractProvider + 'static,
{
    fn id(&self) -> i32 {
        self.id
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "IndexedContract"
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn invoke<D, B>(
        &self,
        _engine: &mut ApplicationEngine<P, D, B>,
        _method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>>
    where
        D: Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        Ok(Vec::new())
    }
}

#[test]
fn resolved_method_preserves_contract_method_index() {
    let contract = IndexedContract {
        id: 42,
        hash: UInt160::zero(),
        methods: vec![
            NativeMethod::new(
                "same",
                1,
                true,
                0,
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_deprecated_in(Hardfork::HfEchidna),
            NativeMethod::new("other", 1, true, 0, vec![], ContractParameterType::Void),
            NativeMethod::new(
                "same",
                2,
                true,
                0,
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfEchidna),
        ],
    };
    let entry = NativeContractsCacheEntry::from_contract::<NoNativeContractProvider, _>(&contract);
    let mut settings = ProtocolSettings::default();
    settings.hardforks = settings.hardforks.with_activation(Hardfork::HfEchidna, 10);

    let before = entry
        .get_method("same", 1, &settings, 9)
        .expect("method resolution succeeds")
        .expect("pre-hardfork method exists");
    assert_eq!(before.method_index(), 0);
    assert_eq!(before.method().cpu_fee, 1);

    let after = entry
        .get_method("same", 1, &settings, 10)
        .expect("method resolution succeeds")
        .expect("post-hardfork method exists");
    assert_eq!(after.method_index(), 2);
    assert_eq!(after.method().cpu_fee, 2);
}

#[test]
fn cache_key_includes_hash_to_avoid_stale_provider_metadata() {
    let mut cache = NativeContractsCache::default();
    let settings = ProtocolSettings::default();
    let first = IndexedContract {
        id: 42,
        hash: UInt160::from_bytes(&[0x11; UInt160::LENGTH]).expect("first hash"),
        methods: vec![NativeMethod::new(
            "first",
            1,
            true,
            0,
            Vec::new(),
            ContractParameterType::Void,
        )],
    };
    let second = IndexedContract {
        id: 42,
        hash: UInt160::from_bytes(&[0x22; UInt160::LENGTH]).expect("second hash"),
        methods: vec![NativeMethod::new(
            "second",
            2,
            true,
            0,
            Vec::new(),
            ContractParameterType::Void,
        )],
    };

    {
        let entry = cache.get_or_build::<NoNativeContractProvider, _>(&first);
        assert!(
            entry
                .get_method("first", 0, &settings, 0)
                .unwrap()
                .is_some()
        );
    }

    let entry = cache.get_or_build::<NoNativeContractProvider, _>(&second);
    assert!(
        entry
            .get_method("second", 0, &settings, 0)
            .unwrap()
            .is_some(),
        "same-id native contracts with different hashes must not reuse stale metadata"
    );
    assert!(
        entry
            .get_method("first", 0, &settings, 0)
            .unwrap()
            .is_none(),
        "second cache entry should be keyed by its own native hash"
    );
}
