use crate::{value::VmValue, VmError};

pub trait NativeInvoker {
    fn invoke(
        &mut self,
        contract: &str,
        method: &str,
        args: &[VmValue],
    ) -> Result<VmValue, VmError>;
}
