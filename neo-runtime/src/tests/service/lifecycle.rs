use super::*;

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
fn lifecycle_traits_accept_concrete_handlers() {
    fn accept_committed<H: CommittedHandler>(_handler: &H) {}
    fn accept_wallet_changed<H: WalletChangedHandler<Sender = (), Wallet = ()>>(_handler: &H) {}

    accept_committed(&NoopCommitted);
    accept_wallet_changed(&NoopWalletChanged);
}
