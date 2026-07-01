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

struct MockAccount {
    hash: UInt160,
    locked: bool,
}
impl AccountLike for MockAccount {
    fn script_hash(&self) -> UInt160 {
        self.hash
    }
    fn is_locked(&self) -> bool {
        self.locked
    }
    fn has_key(&self) -> bool {
        true
    }
    fn get_key(&self) -> Option<Vec<u8>> {
        Some(vec![1, 2, 3])
    }
}

#[test]
fn account_like_dispatches_through_dyn() {
    let hash = UInt160::from_bytes(&[4u8; 20]).unwrap();
    let acct: Arc<dyn AccountLike> = Arc::new(MockAccount { hash, locked: true });
    assert_eq!(acct.script_hash(), hash);
    assert!(acct.is_locked());
    assert!(acct.has_key());
    assert_eq!(acct.get_key(), Some(vec![1, 2, 3]));
}

struct MockMessage {
    cmd: u8,
    data: Vec<u8>,
}
impl MessageLike for MockMessage {
    fn payload(&self) -> &[u8] {
        &self.data
    }
    fn command(&self) -> u8 {
        self.cmd
    }
}

struct DropEverything;
impl MessageReceivedHandler for DropEverything {
    fn remote_node_message_received_handler(
        &self,
        _system: &dyn Any,
        _message: &dyn MessageLike,
    ) -> bool {
        false
    }
}

#[test]
fn message_handler_dispatches_through_dyn() {
    let handler: Arc<dyn MessageReceivedHandler> = Arc::new(DropEverything);
    let msg = MockMessage {
        cmd: 0x2b,
        data: vec![9, 9],
    };
    assert_eq!(msg.command(), 0x2b);
    assert_eq!(msg.payload(), &[9, 9]);
    // The handler drops the message (returns false), invoked via the trait
    // object with a type-erased `&dyn Any` system handle.
    assert!(!handler.remote_node_message_received_handler(&(), &msg));
}

struct MockWallet {
    account: UInt160,
}
impl SignerProvider for MockWallet {
    fn get_account(&self, script_hash: &UInt160) -> Option<Arc<dyn AccountLike>> {
        (*script_hash == self.account).then(|| {
            Arc::new(MockAccount {
                hash: self.account,
                locked: false,
            }) as Arc<dyn AccountLike>
        })
    }
    fn sign(&self, data: &[u8], _script_hash: &UInt160) -> Result<Vec<u8>, String> {
        Ok(data.to_vec())
    }
    fn contains(&self, script_hash: &UInt160) -> bool {
        *script_hash == self.account
    }
}

#[test]
fn wallet_provider_lookup_and_sign() {
    let acct = UInt160::from_bytes(&[5u8; 20]).unwrap();
    let other = UInt160::from_bytes(&[6u8; 20]).unwrap();
    let wallet: Arc<dyn SignerProvider> = Arc::new(MockWallet { account: acct });
    assert!(wallet.contains(&acct));
    assert!(!wallet.contains(&other));
    assert!(wallet.get_account(&acct).is_some());
    assert!(wallet.get_account(&other).is_none());
    assert_eq!(wallet.sign(b"hi", &acct).unwrap(), b"hi");
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
