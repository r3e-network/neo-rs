use super::*;

struct NamedProvider {
    name: &'static str,
}

impl NativeContractProvider for NamedProvider {
    fn get_native_contract(&self, _hash: &UInt160) -> Option<Arc<dyn NativeContract>> {
        None
    }

    fn get_native_contract_by_name(&self, name: &str) -> Option<Arc<dyn NativeContract>> {
        (name == self.name).then(|| Arc::new(NoopNativeContract) as Arc<dyn NativeContract>)
    }

    fn all_native_contracts(&self) -> Vec<Arc<dyn NativeContract>> {
        Vec::new()
    }

    fn all_native_contract_hashes(&self) -> Vec<UInt160> {
        Vec::new()
    }
}

struct NoopNativeContract;

impl NativeContract for NoopNativeContract {
    fn id(&self) -> i32 {
        0
    }

    fn hash(&self) -> UInt160 {
        UInt160::zero()
    }

    fn name(&self) -> &str {
        "Noop"
    }

    fn methods(&self) -> &[crate::NativeMethod] {
        &[]
    }

    fn invoke(
        &self,
        _engine: &mut crate::ApplicationEngine,
        _method: &str,
        _args: &[Vec<u8>],
    ) -> neo_error::CoreResult<Vec<u8>> {
        Ok(Vec::new())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn scoped_provider_overrides_are_thread_local_and_restore_nested_state() {
    let global = Arc::new(NamedProvider { name: "global" });
    let outer = Arc::new(NamedProvider { name: "outer" });
    let inner = Arc::new(NamedProvider { name: "inner" });
    let previous = NativeContractLookup::replace_provider(Some(global));

    NativeContractLookup::with_scoped_provider(outer, || {
        let provider = NativeContractLookup::native_contract_provider().expect("outer scope");
        assert!(provider.get_native_contract_by_name("outer").is_some());
        assert!(provider.get_native_contract_by_name("global").is_none());

        NativeContractLookup::with_scoped_provider(inner, || {
            let provider = NativeContractLookup::native_contract_provider().expect("inner scope");
            assert!(provider.get_native_contract_by_name("inner").is_some());
            assert!(provider.get_native_contract_by_name("outer").is_none());
        });

        let provider = NativeContractLookup::native_contract_provider().expect("outer scope");
        assert!(provider.get_native_contract_by_name("outer").is_some());
        assert!(provider.get_native_contract_by_name("inner").is_none());
    });

    let provider = NativeContractLookup::native_contract_provider().expect("global provider");
    assert!(provider.get_native_contract_by_name("global").is_some());
    NativeContractLookup::replace_provider(previous);
}
