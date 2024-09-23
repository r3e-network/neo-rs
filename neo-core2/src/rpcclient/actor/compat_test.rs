#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpcclient::{WSClient, Client};
    use crate::rpcclient::actor::RPCActor;

    #[test]
    fn test_rpc_actor_rpc_client_compat() {
        let _ = RPCActor::from(WSClient::new());
        let _ = RPCActor::from(Client::new());
    }
}
