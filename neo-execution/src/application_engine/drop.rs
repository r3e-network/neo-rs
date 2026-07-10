use super::*;

impl<P, D, B> Drop for ApplicationEngine<P, D, B>
where
    P: crate::native_contract_provider::NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
{
    fn drop(&mut self) {
        self.vm_engine.engine_mut().clear_interop_host();
        self.diagnostic.disposed();
    }
}
