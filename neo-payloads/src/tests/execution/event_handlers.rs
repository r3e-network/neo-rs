use super::*;

#[test]
fn debug_does_not_leak_system_reference() {
    let event = PluginEvent::NodeStarted {
        system: Arc::new(()) as Arc<dyn Any + Send + Sync>,
    };
    let formatted = format!("{:?}", event);
    assert!(formatted.contains("NodeStarted"));
    assert!(!formatted.contains("()"));
}

#[test]
fn plugin_event_display_includes_block_context() {
    let event = PluginEvent::BlockReceived {
        block_hash: "0xabcd".to_string(),
        block_height: 42,
    };
    let formatted = format!("{}", event);
    assert!(formatted.contains("BlockReceived"));
    assert!(formatted.contains("0xabcd"));
    assert!(formatted.contains("42"));
}

struct NoopCommitted;
impl CommittedHandler for NoopCommitted {
    fn blockchain_committed_handler(&self, _system: &dyn Any, _block: &Block) {}
}
struct NoopWalletChanged;
impl WalletChangedHandler for NoopWalletChanged {
    fn wallet_provider_wallet_changed_handler(
        &self,
        _sender: &dyn Any,
        _wallet: Option<Arc<dyn Any + Send + Sync>>,
    ) {
    }
}

#[test]
fn lifecycle_handler_traits_are_object_safe() {
    // Constructing the trait objects confirms object-safety (these are used
    // as `dyn` handlers by plugins/services).
    let _committed: Arc<dyn CommittedHandler> = Arc::new(NoopCommitted);
    let _wallet_changed: Arc<dyn WalletChangedHandler> = Arc::new(NoopWalletChanged);
}
