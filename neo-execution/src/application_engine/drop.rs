use super::*;

impl Drop for ApplicationEngine {
    fn drop(&mut self) {
        self.vm_engine.engine_mut().clear_interop_host();
        if let Some(diagnostic) = self.diagnostic.as_mut() {
            diagnostic.disposed();
        }
    }
}
