//
// interop.rs - Interop service and host methods
//

use super::{ExecutionEngine, HostPtr, InteropHost, InteropService, VmError, VmResult};

impl ExecutionEngine {
    /// Sets the interop service used for syscall dispatch.
    pub fn set_interop_service(&mut self, service: InteropService) {
        self.interop_service = Some(service);
    }

    /// Clears the currently assigned interop service.
    pub fn clear_interop_service(&mut self) {
        self.interop_service = None;
    }

    /// Returns a reference to the configured interop service, if any.
    #[must_use]
    pub const fn interop_service(&self) -> Option<&InteropService> {
        self.interop_service.as_ref()
    }

    /// Returns a mutable reference to the configured interop service, if any.
    pub fn interop_service_mut(&mut self) -> Option<&mut InteropService> {
        self.interop_service.as_mut()
    }

    /// Assigns the host responsible for advanced interop handling.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `host` remains valid for the lifetime of this
    /// `ExecutionEngine` and that no aliasing `&mut` references exist during callbacks.
    pub unsafe fn set_interop_host(&mut self, host: *mut dyn InteropHost) {
        // SAFETY: The caller is responsible for upholding the HostPtr invariants.
        self.interop_host = Some(unsafe { HostPtr::new(host) });
    }

    /// Clears the registered interop host.
    pub fn clear_interop_host(&mut self) {
        self.interop_host = None;
    }

    /// Returns a mutable reference to the configured interop host, if any.
    pub fn interop_host_mut(&mut self) -> Option<&mut dyn InteropHost> {
        self.interop_host.map(|h| {
            // SAFETY: The HostPtr invariants guarantee the pointer is valid and
            // exclusively accessible. The returned reference borrows `self` via
            // the function signature, preventing aliasing.
            unsafe { &mut *h.as_raw() }
        })
    }

    /// Returns the raw pointer to the configured interop host, if any.
    #[must_use]
    pub fn interop_host_ptr(&self) -> Option<*mut dyn InteropHost> {
        self.interop_host.map(|h| h.as_raw())
    }

    /// Invokes a syscall on the configured interop host.
    ///
    /// Returns `Some(result)` if a host was present and the call was dispatched,
    /// `None` if no host is configured.
    pub fn invoke_host_syscall(&mut self, hash: u32) -> Option<VmResult<()>> {
        if let Some(host) = self.interop_host {
            Some(host.invoke_syscall(self, hash))
        } else {
            None
        }
    }

    /// Invokes the CALLT opcode by delegating to the interop host.
    ///
    /// This method is called by the CALLT instruction handler to resolve method tokens
    /// and perform cross-contract calls via the `ApplicationEngine`.
    pub fn invoke_callt(&mut self, token_id: u16) -> VmResult<()> {
        if let Some(host) = self.interop_host {
            host.on_callt(self, token_id)
        } else {
            Err(VmError::invalid_operation_msg(format!(
                "CALLT (token {token_id}) requires ApplicationEngine context. \
                 No interop host configured."
            )))
        }
    }
}
