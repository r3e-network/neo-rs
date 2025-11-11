use std::{future::Future, sync::Arc};

use dashmap::DashMap;
use futures::future::{BoxFuture, FutureExt};
use serde_json::Value;

use crate::{
    params::RpcParams,
    request::RpcRequest,
    response::{RpcError, RpcResponse, JSONRPC_VERSION},
};

pub type RpcResult = Result<Value, RpcError>;

type RpcMethod = Arc<dyn Send + Sync + Fn(RpcParams) -> BoxFuture<'static, RpcResult>>;

pub struct RpcModule {
    methods: DashMap<String, RpcMethod>,
}

impl RpcModule {
    pub fn new() -> Self {
        Self {
            methods: DashMap::new(),
        }
    }

    pub fn register<F, Fut>(&self, name: &str, func: F)
    where
        F: Send + Sync + 'static + Fn(RpcParams) -> Fut,
        Fut: Future<Output = RpcResult> + Send + 'static,
    {
        let handler = Arc::new(move |params: RpcParams| func(params).boxed()) as RpcMethod;
        self.methods.insert(name.to_string(), handler);
    }

    pub async fn call(&self, request: RpcRequest) -> RpcResponse {
        if !request.is_version_valid() {
            return RpcResponse::error(
                request.id,
                RpcError::invalid_request(format!("jsonrpc must be \"{}\"", JSONRPC_VERSION)),
            );
        }
        let method = match self.methods.get(&request.method) {
            Some(handler) => handler.clone(),
            None => {
                return RpcResponse::error(request.id, RpcError::method_not_found(&request.method));
            }
        };
        let params = RpcParams::new(request.params);
        match method(params).await {
            Ok(result) => RpcResponse::result(request.id, result),
            Err(err) => RpcResponse::error(request.id, err),
        }
    }
}

pub async fn handle_single_request(module: &RpcModule, value: Value) -> RpcResponse {
    match serde_json::from_value::<RpcRequest>(value) {
        Ok(request) => module.call(request).await,
        Err(err) => RpcResponse::error(Value::Null, RpcError::invalid_request(err.to_string())),
    }
}
