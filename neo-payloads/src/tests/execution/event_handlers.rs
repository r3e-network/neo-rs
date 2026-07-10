use super::*;

#[test]
fn debug_does_not_leak_system_reference() {
    let event = PluginEvent::NodeStarted {
        system: Arc::new(()),
    };
    let formatted = format!("{:?}", event);
    assert!(formatted.contains("NodeStarted"));
    assert!(!formatted.contains("()"));
}

#[test]
fn plugin_event_display_includes_block_context() {
    let event: PluginEvent = PluginEvent::BlockReceived {
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
    fn blockchain_committed_handler(&self, _network: u32, _block: &Block) {}
}
struct NoopWalletChanged;
impl WalletChangedHandler for NoopWalletChanged {
    type Sender = ();
    type Wallet = ();

    fn wallet_provider_wallet_changed_handler(
        &self,
        _sender: &Self::Sender,
        _wallet: Option<Arc<Self::Wallet>>,
    ) {
    }
}

#[test]
fn lifecycle_handler_traits_accept_concrete_handlers() {
    fn accept_committed<H: CommittedHandler>(_handler: &H) {}
    fn accept_wallet_changed<H: WalletChangedHandler<Sender = (), Wallet = ()>>(_handler: &H) {}

    let committed = NoopCommitted;
    let wallet_changed = NoopWalletChanged;
    accept_committed(&committed);
    accept_wallet_changed(&wallet_changed);
}
