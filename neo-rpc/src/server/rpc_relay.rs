use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;
use async_trait::async_trait;
use neo_core::akka::{Actor, ActorContext, ActorRef, ActorResult, ActorSystem, Props};
use neo_core::ledger::{RelayResult, VerifyResult};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::any::Any;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;

pub(super) fn map_relay_result(result: RelayResult) -> Result<Value, RpcException> {
    match result.result {
        VerifyResult::Succeed => Ok(json!({ "hash": result.hash.to_string() })),
        VerifyResult::AlreadyExists => Err(RpcException::from(RpcError::already_exists())),
        VerifyResult::AlreadyInPool => Err(RpcException::from(RpcError::already_in_pool())),
        VerifyResult::OutOfMemory => Err(RpcException::from(RpcError::mempool_cap_reached())),
        VerifyResult::InvalidScript => Err(RpcException::from(RpcError::invalid_script())),
        VerifyResult::InvalidAttribute => Err(RpcException::from(RpcError::invalid_attribute())),
        VerifyResult::InvalidSignature => Err(RpcException::from(RpcError::invalid_signature())),
        VerifyResult::OverSize => Err(RpcException::from(RpcError::invalid_size())),
        VerifyResult::Expired => Err(RpcException::from(RpcError::expired_transaction())),
        VerifyResult::InsufficientFunds => Err(RpcException::from(RpcError::insufficient_funds())),
        VerifyResult::PolicyFail => Err(RpcException::from(RpcError::policy_failed())),
        VerifyResult::UnableToVerify => Err(RpcException::from(
            RpcError::verification_failed().with_data("UnableToVerify"),
        )),
        VerifyResult::Invalid => Err(RpcException::from(
            RpcError::verification_failed().with_data("Invalid"),
        )),
        VerifyResult::HasConflicts => Err(RpcException::from(
            RpcError::verification_failed().with_data("HasConflicts"),
        )),
        VerifyResult::Unknown => Err(RpcException::from(
            RpcError::verification_failed().with_data("Unknown"),
        )),
    }
}

pub(super) fn with_relay_responder<F>(
    server: &RpcServer,
    send: F,
) -> Result<RelayResult, RpcException>
where
    F: FnOnce(ActorRef) -> Result<(), RpcException>,
{
    let system = server.system();
    let actor_system = system.actor_system();
    let (responder, rx) = spawn_relay_responder(actor_system)?;
    if let Err(err) = send(responder.clone()) {
        let _ = actor_system.stop(&responder);
        return Err(err);
    }
    let result = rx
        .blocking_recv()
        .map_err(|_| internal_error("relay result channel closed"))?;
    let _ = actor_system.stop(&responder);
    Ok(result)
}

fn spawn_relay_responder(
    actor_system: &ActorSystem,
) -> Result<(ActorRef, oneshot::Receiver<RelayResult>), RpcException> {
    let (tx, rx) = oneshot::channel();
    let completion = Arc::new(Mutex::new(Some(tx)));
    let props = {
        let completion = completion;
        Props::new(move || RelayResultResponder {
            completion: completion.clone(),
        })
    };
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let actor = actor_system
        .actor_of(props, format!("rpc-relay-{unique}"))
        .map_err(|err| internal_error(err.to_string()))?;
    Ok((actor, rx))
}

struct RelayResultResponder {
    completion: Arc<Mutex<Option<oneshot::Sender<RelayResult>>>>,
}

#[async_trait]
impl Actor for RelayResultResponder {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        if let Ok(result) = message.downcast::<RelayResult>() {
            let mut guard = self.completion.lock();
            if let Some(tx) = guard.take() {
                let _ = tx.send(*result);
            }
            let _ = ctx.stop_self();
        }
        Ok(())
    }
}
