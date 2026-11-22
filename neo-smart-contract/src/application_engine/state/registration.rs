use super::*;
/// Registration surface shared by an application engine and the immutable
/// default-registry builder. Keeping hardfork selection explicit prevents a
/// Faun-only descriptor from leaking into pre-Faun engines.
pub(crate) trait InteropRegistration {
    fn register_host_service(
        &mut self,
        name: &str,
        price: i64,
        call_flags: CallFlags,
        handler: InteropHandler,
    ) -> VmResult<()>;

    /// Registers a host service that also carries an `HF_Huyao` fixed price in
    /// 1e-11 GAS units (C# `InteropPriceAttribute.Coefficient`).
    fn register_host_service_huyao(
        &mut self,
        name: &str,
        price: i64,
        huyao_price: i64,
        call_flags: CallFlags,
        handler: InteropHandler,
    ) -> VmResult<()>;

    fn is_hardfork_enabled(&self, hardfork: Hardfork) -> bool;
}

pub(super) struct DefaultInterops {
    pub(super) service: InteropService,
    pub(super) handlers: Arc<HashMap<u32, HostInteropHandler>>,
}

struct DefaultInteropBuilder {
    service: InteropService,
    handlers: HashMap<u32, HostInteropHandler>,
    faun_enabled: bool,
    huyao_enabled: bool,
}

impl InteropRegistration for DefaultInteropBuilder {
    fn register_host_service(
        &mut self,
        name: &str,
        price: i64,
        call_flags: CallFlags,
        handler: InteropHandler,
    ) -> VmResult<()> {
        let hash = self
            .service
            .register_host_descriptor(name, price, call_flags)?;
        self.handlers.insert(
            hash,
            HostInteropHandler {
                price,
                huyao_price: None,
                required_call_flags: call_flags,
                handler,
            },
        );
        Ok(())
    }

    fn register_host_service_huyao(
        &mut self,
        name: &str,
        price: i64,
        huyao_price: i64,
        call_flags: CallFlags,
        handler: InteropHandler,
    ) -> VmResult<()> {
        let hash = self
            .service
            .register_host_descriptor(name, price, call_flags)?;
        self.handlers.insert(
            hash,
            HostInteropHandler {
                price,
                huyao_price: Some(huyao_price),
                required_call_flags: call_flags,
                handler,
            },
        );
        Ok(())
    }

    fn is_hardfork_enabled(&self, hardfork: Hardfork) -> bool {
        match hardfork {
            Hardfork::HfFaun => self.faun_enabled,
            Hardfork::HfHuyao => self.huyao_enabled,
            _ => false,
        }
    }
}

impl InteropRegistration for ApplicationEngine {
    fn register_host_service(
        &mut self,
        name: &str,
        price: i64,
        call_flags: CallFlags,
        handler: InteropHandler,
    ) -> VmResult<()> {
        ApplicationEngine::register_host_service(self, name, price, call_flags, handler)
    }

    fn register_host_service_huyao(
        &mut self,
        name: &str,
        price: i64,
        huyao_price: i64,
        call_flags: CallFlags,
        handler: InteropHandler,
    ) -> VmResult<()> {
        ApplicationEngine::register_host_service_huyao(
            self,
            name,
            price,
            huyao_price,
            call_flags,
            handler,
        )
    }

    fn is_hardfork_enabled(&self, hardfork: Hardfork) -> bool {
        ApplicationEngine::is_hardfork_enabled(self, hardfork)
    }
}

/// The default registry is cached per `(Faun, Huyao)` hardfork pair: those are
/// the two hardforks that change the *registered syscall surface* (Faun adds
/// `System.Storage.Local.*`, Huyao adds `System.Storage.FindWithStart` and
/// `System.Storage.Local.FindWithStart`). Price-only hardforks stay out of the
/// key on purpose, because their Huyao prices switch per invocation.
static DEFAULT_INTEROPS: [OnceLock<Result<Arc<DefaultInterops>>>; 4] = [
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
];

pub(super) fn default_interops(
    faun_enabled: bool,
    huyao_enabled: bool,
) -> Result<Arc<DefaultInterops>> {
    DEFAULT_INTEROPS[usize::from(faun_enabled) + 2 * usize::from(huyao_enabled)]
        .get_or_init(|| build_default_interops(faun_enabled, huyao_enabled))
        .clone()
}

fn build_default_interops(faun_enabled: bool, huyao_enabled: bool) -> Result<Arc<DefaultInterops>> {
    let mut builder = DefaultInteropBuilder {
        service: InteropService::new(),
        handlers: HashMap::new(),
        faun_enabled,
        huyao_enabled,
    };
    register_default_interops(&mut builder)?;
    Ok(Arc::new(DefaultInterops {
        service: builder.service,
        handlers: Arc::new(builder.handlers),
    }))
}

fn register_default_interops(registry: &mut dyn InteropRegistration) -> Result<()> {
    register_contract_interops(registry)
        .map_err(|err| ApplicationEngine::map_vm_error("System.Contract", err))?;
    register_runtime_interops(registry)
        .map_err(|err| ApplicationEngine::map_vm_error("System.Runtime", err))?;
    register_storage_interops(registry)
        .map_err(|err| ApplicationEngine::map_vm_error("System.Storage", err))?;
    register_iterator_interops(registry)
        .map_err(|err| ApplicationEngine::map_vm_error("System.Iterator", err))?;
    register_crypto_interops(registry)
        .map_err(|err| ApplicationEngine::map_vm_error("System.Crypto", err))?;
    Ok(())
}
