// Copyright (C) 2015-2025 The Neo Project.
//
// rest_web_server.rs ports Neo.Plugins.RestServer.RestWebServer.cs to Rust.
// It leverages warp as the HTTP server while mirroring the structure and
// behaviour of the original ASP.NET Core pipeline for the implemented routes.

use crate::rest_server::authentication::basic_authentication_handler::BasicAuthenticationHandler;
use crate::rest_server::controllers::v1::contracts_controller::ContractsController;
use crate::rest_server::controllers::v1::ledger_controller::LedgerController;
use crate::rest_server::controllers::v1::node_controller::NodeController;
use crate::rest_server::controllers::v1::tokens_controller::TokensController;
use crate::rest_server::controllers::v1::utils_controller::UtilsController;
use crate::rest_server::models::error::error_model::ErrorModel;
use crate::rest_server::providers::black_list_controller_feature_provider::BlackListControllerFeatureProvider;
use crate::rest_server::middleware::rest_server_middleware::RestServerMiddleware;
use crate::rest_server::rest_server_settings::RestServerSettings;
use hyper::header::{HeaderValue, WWW_AUTHENTICATE};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::convert::Infallible;
use tokio::sync::Notify;
use tokio::task::{JoinHandle, JoinError};
use tracing::{info, warn};
use warp::filters::BoxedFilter;
use warp::reject::Reject;
use warp::reply::Response;
use warp::Filter;
use warp::Reply;

static IS_RUNNING: AtomicBool = AtomicBool::new(false);

/// Rust port of the C# RestWebServer class.
pub struct RestWebServer {
    settings: RestServerSettings,
    shutdown: Option<Arc<Notify>>,
    server_task: Option<JoinHandle<()>>,
}

impl RestWebServer {
    /// Creates a new RestWebServer with the currently loaded settings.
    pub fn new() -> Self {
        Self {
            settings: RestServerSettings::current(),
            shutdown: None,
            server_task: None,
        }
    }

    /// Starts the web server (matches C# Start method semantics).
    pub fn start(&mut self) {
        if Self::is_running() {
            return;
        }

        let settings = RestServerSettings::current();
        self.settings = settings.clone();

        if settings.ssl_cert_file.is_some() {
            warn!("SSL certificates are not yet supported in the Rust REST server");
        }

        let address = socket_addr(settings.bind_address, settings.port);
        let routes = Self::build_routes(&settings);
        let shutdown = Arc::new(Notify::new());
        let shutdown_signal = shutdown.clone();

        info!("Starting REST server on {}", address);
        Self::set_running(true);

        let (bound_address, server_future) = warp::serve(routes).bind_with_graceful_shutdown(
            address,
            async move {
                shutdown_signal.notified().await;
            },
        );

        info!("REST server bound on {}", bound_address);

        let server_task = tokio::spawn(async move {
            server_future.await;
        });

        self.shutdown = Some(shutdown);
        self.server_task = Some(server_task);
    }

    /// Stops the web server and waits for the background task to finish.
    pub fn stop(&mut self) {
        if !Self::is_running() {
            return;
        }

        if let Some(shutdown) = self.shutdown.take() {
            shutdown.notify_waiters();
        }

        if let Some(handle) = self.server_task.take() {
            tokio::spawn(async move {
                if let Err(error) = handle.await {
                    log_join_error(error);
                }
            });
        }

        Self::set_running(false);
        info!("REST server stopped");
    }

    /// Returns whether the server is currently running.
    pub fn is_running() -> bool {
        IS_RUNNING.load(Ordering::Relaxed)
    }

    fn set_running(running: bool) {
        IS_RUNNING.store(running, Ordering::Relaxed);
    }

    fn build_routes(_settings: &RestServerSettings) -> BoxedFilter<(Response,)> {
        let mut routes: Vec<BoxedFilter<(Response,)>> = Vec::new();
        let provider = BlackListControllerFeatureProvider::new();

        if provider.is_controller_allowed("Node") {
            routes.push(node_routes());
        }

        if provider.is_controller_allowed("Utils") {
            routes.push(utils_routes());
        }
        if provider.is_controller_allowed("Ledger") {
            routes.push(ledger_routes());
        }

        if provider.is_controller_allowed("Contracts") {
            routes.push(contracts_routes());
        }

        if provider.is_controller_allowed("Tokens") {
            routes.push(tokens_routes());
        }

        let combined = if routes.is_empty() {
            fallback_route()
        } else {
            combine_filters(routes)
        };

        enforce_authentication(combined)
            .recover(handle_rejection)
            .unify()
            .boxed()
    }
}

fn node_routes() -> BoxedFilter<(Response,)> {
    let peers = warp::path!("api" / "v1" / "node" / "peers")
        .and(warp::path::end())
        .map(handle_get_peers)
        .boxed();

    let plugins = warp::path!("api" / "v1" / "node" / "plugins")
        .and(warp::path::end())
        .map(handle_get_plugins)
        .boxed();

    let settings = warp::path!("api" / "v1" / "node" / "settings")
        .and(warp::path::end())
        .map(handle_get_settings)
        .boxed();

    combine_filters(vec![peers, plugins, settings])
}

fn utils_routes() -> BoxedFilter<(Response,)> {
    let script_hash_to_address = warp::path!("api" / "v1" / "utils" / String / "address")
        .and(warp::path::end())
        .map(handle_script_hash_to_address)
        .boxed();

    let address_to_script_hash = warp::path!("api" / "v1" / "utils" / String / "scripthash")
        .and(warp::path::end())
        .map(handle_address_to_script_hash)
        .boxed();

    let validate_address = warp::path!("api" / "v1" / "utils" / String / "validate")
        .and(warp::path::end())
        .map(handle_validate_address)
        .boxed();

    combine_filters(vec![
        script_hash_to_address,
        address_to_script_hash,
        validate_address,
    ])
}

fn ledger_routes() -> BoxedFilter<(Response,)> {
    let gas_accounts = warp::path!("api" / "v1" / "ledger" / "gas" / "accounts")
        .and(warp::query::<PaginationQuery>())
        .map(handle_get_gas_accounts)
        .boxed();

    let neo_accounts = warp::path!("api" / "v1" / "ledger" / "neo" / "accounts")
        .and(warp::query::<PaginationQuery>())
        .map(handle_get_neo_accounts)
        .boxed();

    let blocks = warp::path!("api" / "v1" / "ledger" / "blocks")
        .and(warp::query::<PaginationQuery>())
        .map(handle_get_blocks)
        .boxed();

    let current_block_header = warp::path!("api" / "v1" / "ledger" / "blockheader" / "current")
        .and(warp::path::end())
        .map(handle_get_current_block_header)
        .boxed();

    let block = warp::path!("api" / "v1" / "ledger" / "blocks" / u32)
        .and(warp::path::end())
        .map(handle_get_block)
        .boxed();

    let block_header = warp::path!("api" / "v1" / "ledger" / "blocks" / u32 / "header")
        .and(warp::path::end())
        .map(handle_get_block_header)
        .boxed();

    let block_witness = warp::path!("api" / "v1" / "ledger" / "blocks" / u32 / "witness")
        .and(warp::path::end())
        .map(handle_get_block_witness)
        .boxed();

    let block_transactions = warp::path!("api" / "v1" / "ledger" / "blocks" / u32 / "transactions")
        .and(warp::query::<PaginationQuery>())
        .map(handle_get_block_transactions)
        .boxed();

    let transaction = warp::path!("api" / "v1" / "ledger" / "transactions" / String)
        .and(warp::path::end())
        .map(handle_get_transaction)
        .boxed();

    let transaction_witnesses =
        warp::path!("api" / "v1" / "ledger" / "transactions" / String / "witnesses")
            .and(warp::path::end())
            .map(handle_get_transaction_witnesses)
            .boxed();

    let transaction_signers =
        warp::path!("api" / "v1" / "ledger" / "transactions" / String / "signers")
            .and(warp::path::end())
            .map(handle_get_transaction_signers)
            .boxed();

    let transaction_attributes =
        warp::path!("api" / "v1" / "ledger" / "transactions" / String / "attributes")
            .and(warp::path::end())
            .map(handle_get_transaction_attributes)
            .boxed();

    let mempool = warp::path!("api" / "v1" / "ledger" / "memorypool")
        .and(warp::query::<PaginationQuery>())
        .map(handle_get_memory_pool)
        .boxed();

    let mempool_verified =
        warp::path!("api" / "v1" / "ledger" / "memorypool" / "verified")
            .and(warp::query::<PaginationQuery>())
            .map(handle_get_memory_pool_verified)
            .boxed();

    let mempool_unverified =
        warp::path!("api" / "v1" / "ledger" / "memorypool" / "unverified")
            .and(warp::query::<PaginationQuery>())
            .map(handle_get_memory_pool_unverified)
            .boxed();

    let mempool_counts = warp::path!("api" / "v1" / "ledger" / "memorypool" / "counts")
        .and(warp::path::end())
        .map(handle_get_memory_pool_counts)
        .boxed();

    combine_filters(vec![
        gas_accounts,
        neo_accounts,
        blocks,
        current_block_header,
        block,
        block_header,
        block_witness,
        block_transactions,
        transaction,
        transaction_witnesses,
        transaction_signers,
        transaction_attributes,
        mempool,
        mempool_verified,
        mempool_unverified,
        mempool_counts,
    ])
}

fn contracts_routes() -> BoxedFilter<(Response,)> {
    let contracts = warp::path!("api" / "v1" / "contracts")
        .and(warp::query::<PaginationQuery>())
        .and(warp::path::end())
        .map(handle_get_contracts)
        .boxed();

    let count = warp::path!("api" / "v1" / "contracts" / "count")
        .and(warp::path::end())
        .map(handle_get_contracts_count)
        .boxed();

    let contract = warp::path!("api" / "v1" / "contracts" / String)
        .and(warp::path::end())
        .map(handle_get_contract)
        .boxed();

    let manifest = warp::path!("api" / "v1" / "contracts" / String / "manifest")
        .and(warp::path::end())
        .map(handle_get_contract_manifest)
        .boxed();

    let abi = warp::path!("api" / "v1" / "contracts" / String / "abi")
        .and(warp::path::end())
        .map(handle_get_contract_abi)
        .boxed();

    let nef = warp::path!("api" / "v1" / "contracts" / String / "nef")
        .and(warp::path::end())
        .map(handle_get_contract_nef)
        .boxed();

    let storage = warp::path!("api" / "v1" / "contracts" / String / "storage")
        .and(warp::path::end())
        .map(handle_get_contract_storage)
        .boxed();

    let invoke = warp::path!("api" / "v1" / "contracts" / String / "invoke")
        .and(warp::query::<InvokeQuery>())
        .and(warp::body::json())
        .map(handle_invoke_contract)
        .boxed();

    combine_filters(vec![
        contracts,
        count,
        contract,
        manifest,
        abi,
        nef,
        storage,
        invoke,
    ])
}

fn tokens_routes() -> BoxedFilter<(Response,)> {
    let nep17_tokens = warp::path!("api" / "v1" / "tokens" / "nep-17")
        .and(warp::query::<PaginationQuery>())
        .map(handle_get_nep17_tokens)
        .boxed();

    let nep17_balance = warp::path!("api" / "v1" / "tokens" / "nep-17" / String / "balanceof" / String)
        .and(warp::path::end())
        .map(handle_get_nep17_balance)
        .boxed();

    let nep11_tokens = warp::path!("api" / "v1" / "tokens" / "nep-11")
        .and(warp::query::<PaginationQuery>())
        .map(handle_get_nep11_tokens)
        .boxed();

    let nep11_balance = warp::path!("api" / "v1" / "tokens" / "nep-11" / String / "balanceof" / String)
        .and(warp::path::end())
        .map(handle_get_nep11_balance)
        .boxed();

    let all_balances = warp::path!("api" / "v1" / "tokens" / "balanceof" / String)
        .and(warp::path::end())
        .map(handle_get_all_balances)
        .boxed();

    combine_filters(vec![
        nep17_tokens,
        nep17_balance,
        nep11_tokens,
        nep11_balance,
        all_balances,
    ])
}

fn handle_get_contracts(query: PaginationQuery) -> Response {
    match ContractsController::new() {
        Ok(controller) => {
            response_from_optional_json(controller.list(query.page, query.size))
        }
        Err(error) => error_response(error),
    }
}

fn handle_get_contracts_count() -> Response {
    match ContractsController::new() {
        Ok(controller) => response_from_json(controller.count()),
        Err(error) => error_response(error),
    }
}

fn handle_get_contract(script_hash: String) -> Response {
    match ContractsController::parse_script_hash(&script_hash) {
        Ok(hash) => match ContractsController::new() {
            Ok(controller) => response_from_json(controller.contract(&hash)),
            Err(error) => error_response(error),
        },
        Err(error) => error_response(error),
    }
}

fn handle_get_contract_manifest(script_hash: String) -> Response {
    match ContractsController::parse_script_hash(&script_hash) {
        Ok(hash) => match ContractsController::new() {
            Ok(controller) => response_from_json(controller.manifest(&hash)),
            Err(error) => error_response(error),
        },
        Err(error) => error_response(error),
    }
}

fn handle_get_contract_abi(script_hash: String) -> Response {
    match ContractsController::parse_script_hash(&script_hash) {
        Ok(hash) => match ContractsController::new() {
            Ok(controller) => response_from_json(controller.abi(&hash)),
            Err(error) => error_response(error),
        },
        Err(error) => error_response(error),
    }
}

fn handle_get_contract_nef(script_hash: String) -> Response {
    match ContractsController::parse_script_hash(&script_hash) {
        Ok(hash) => match ContractsController::new() {
            Ok(controller) => response_from_json(controller.nef(&hash)),
            Err(error) => error_response(error),
        },
        Err(error) => error_response(error),
    }
}

fn handle_get_contract_storage(script_hash: String) -> Response {
    match ContractsController::parse_script_hash(&script_hash) {
        Ok(hash) => match ContractsController::new() {
            Ok(controller) => {
                response_from_optional_json(controller.storage(&hash))
            }
            Err(error) => error_response(error),
        },
        Err(error) => error_response(error),
    }
}

fn handle_invoke_contract(script_hash: String, query: InvokeQuery, payload: Value) -> Response {
    match ContractsController::parse_script_hash(&script_hash) {
        Ok(hash) => match ContractsController::new() {
            Ok(controller) => {
                let method = query.method.unwrap_or_default();
                response_from_json(
                    controller.invoke_contract(&hash, &method, &payload),
                )
            }
            Err(error) => error_response(error),
        },
        Err(error) => error_response(error),
    }
}

fn handle_get_peers() -> Response {
    match NodeController::new() {
        Ok(controller) => response_from_result(controller.get_peers()),
        Err(error) => error_response(error),
    }
}

fn handle_get_plugins() -> Response {
    match NodeController::new() {
        Ok(controller) => response_from_result(controller.get_plugins()),
        Err(error) => error_response(error),
    }
}

fn handle_get_settings() -> Response {
    match NodeController::new() {
        Ok(controller) => response_from_result(controller.get_settings()),
        Err(error) => error_response(error),
    }
}

fn handle_get_nep17_tokens(query: PaginationQuery) -> Response {
    match TokensController::new() {
        Ok(controller) => response_from_optional_result(controller.get_nep17(query.page, query.size)),
        Err(error) => error_response(error),
    }
}

fn handle_get_nep17_balance(script_hash: String, address: String) -> Response {
    match TokensController::new() {
        Ok(controller) => response_from_result(controller.get_nep17_balance_of(&script_hash, &address)),
        Err(error) => error_response(error),
    }
}

fn handle_get_nep11_tokens(query: PaginationQuery) -> Response {
    match TokensController::new() {
        Ok(controller) => response_from_optional_result(controller.get_nep11(query.page, query.size)),
        Err(error) => error_response(error),
    }
}

fn handle_get_nep11_balance(script_hash: String, address: String) -> Response {
    match TokensController::new() {
        Ok(controller) => response_from_result(controller.get_nep11_balance_of(&script_hash, &address)),
        Err(error) => error_response(error),
    }
}

fn handle_get_all_balances(address: String) -> Response {
    match TokensController::new() {
        Ok(controller) => response_from_result(controller.get_balances(&address)),
        Err(error) => error_response(error),
    }
}

fn handle_get_gas_accounts(query: PaginationQuery) -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_optional_json(controller.gas_accounts(query.page, query.size)),
        Err(error) => error_response(error),
    }
}

fn handle_get_neo_accounts(query: PaginationQuery) -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_optional_json(controller.neo_accounts(query.page, query.size)),
        Err(error) => error_response(error),
    }
}

fn handle_get_blocks(query: PaginationQuery) -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_optional_json(controller.blocks(query.page, query.size)),
        Err(error) => error_response(error),
    }
}

fn handle_get_current_block_header() -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_json(controller.current_block_header()),
        Err(error) => error_response(error),
    }
}

fn handle_get_block(index: u32) -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_json(controller.block(index)),
        Err(error) => error_response(error),
    }
}

fn handle_get_block_header(index: u32) -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_json(controller.block_header(index)),
        Err(error) => error_response(error),
    }
}

fn handle_get_block_witness(index: u32) -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_json(controller.block_witness(index)),
        Err(error) => error_response(error),
    }
}

fn handle_get_block_transactions(index: u32, query: PaginationQuery) -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_optional_json(controller.block_transactions(index, query.page, query.size)),
        Err(error) => error_response(error),
    }
}

fn handle_get_transaction(hash: String) -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_json(controller.transaction(&hash)),
        Err(error) => error_response(error),
    }
}

fn handle_get_transaction_witnesses(hash: String) -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_json(controller.transaction_witnesses(&hash)),
        Err(error) => error_response(error),
    }
}

fn handle_get_transaction_signers(hash: String) -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_json(controller.transaction_signers(&hash)),
        Err(error) => error_response(error),
    }
}

fn handle_get_transaction_attributes(hash: String) -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_json(controller.transaction_attributes(&hash)),
        Err(error) => error_response(error),
    }
}

fn handle_get_memory_pool(query: PaginationQuery) -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_json(controller.memory_pool(query.page, query.size)),
        Err(error) => error_response(error),
    }
}

fn handle_get_memory_pool_verified(query: PaginationQuery) -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_optional_json(controller.memory_pool_verified(query.page, query.size)),
        Err(error) => error_response(error),
    }
}

fn handle_get_memory_pool_unverified(query: PaginationQuery) -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_optional_json(controller.memory_pool_unverified(query.page, query.size)),
        Err(error) => error_response(error),
    }
}

fn handle_get_memory_pool_counts() -> Response {
    match LedgerController::new() {
        Ok(controller) => response_from_result(controller.memory_pool_counts()),
        Err(error) => error_response(error),
    }
}

fn handle_script_hash_to_address(hash: String) -> Response {
    match UtilsController::new() {
        Ok(controller) => response_from_result(controller.script_hash_to_wallet_address(&hash)),
        Err(error) => error_response(error),
    }
}

fn handle_address_to_script_hash(address: String) -> Response {
    match UtilsController::new() {
        Ok(controller) => response_from_result(controller.wallet_address_to_script_hash(&address)),
        Err(error) => error_response(error),
    }
}

fn handle_validate_address(address: String) -> Response {
    match UtilsController::new() {
        Ok(controller) => response_from_result(controller.validate_address(&address)),
        Err(error) => error_response(error),
    }
}

fn enforce_authentication(
    routes: BoxedFilter<(Response,)>,
) -> impl Filter<Extract = (Response,), Error = warp::Rejection> + Clone {
    basic_auth_filter()
        .and(routes)
        .map(|response: Response| response)
}

fn basic_auth_filter() -> BoxedFilter<()> {
    warp::header::optional::<String>("authorization")
        .and_then(|header: Option<String>| async move {
            if BasicAuthenticationHandler::authenticate(header.as_deref()) {
                Ok(())
            } else {
                Err(warp::reject::custom(Unauthorized))
            }
        })
        .untuple_one()
        .boxed()
}

fn response_from_result<T>(result: Result<T, ErrorModel>) -> Response
where
    T: Serialize,
{
    let mut response = match result {
        Ok(value) => warp::reply::with_status(warp::reply::json(&value), StatusCode::OK).into_response(),
        Err(error) => error_response(error),
    };
    RestServerMiddleware::set_server_information_header(&mut response);
    response
}

fn error_response(error: ErrorModel) -> Response {
    let mut response = warp::reply::with_status(warp::reply::json(&error), StatusCode::BAD_REQUEST).into_response();
    RestServerMiddleware::set_server_information_header(&mut response);
    response
}

fn response_from_json(result: Result<Value, ErrorModel>) -> Response {
    let mut response = match result {
        Ok(value) => warp::reply::with_status(warp::reply::json(&value), StatusCode::OK).into_response(),
        Err(error) => error_response(error),
    };
    RestServerMiddleware::set_server_information_header(&mut response);
    response
}

fn response_from_optional_json(result: Result<Option<Value>, ErrorModel>) -> Response {
    let mut response = match result {
        Ok(Some(value)) => warp::reply::with_status(warp::reply::json(&value), StatusCode::OK).into_response(),
        Ok(None) => warp::reply::with_status(warp::reply(), StatusCode::NO_CONTENT).into_response(),
        Err(error) => error_response(error),
    };
    RestServerMiddleware::set_server_information_header(&mut response);
    response
}

fn response_from_optional_result<T>(result: Result<Option<T>, ErrorModel>) -> Response
where
    T: Serialize,
{
    let mut response = match result {
        Ok(Some(value)) => warp::reply::with_status(warp::reply::json(&value), StatusCode::OK)
            .into_response(),
        Ok(None) => warp::reply::with_status(warp::reply(), StatusCode::NO_CONTENT).into_response(),
        Err(error) => error_response(error),
    };
    RestServerMiddleware::set_server_information_header(&mut response);
    response
}

fn fallback_route() -> BoxedFilter<(Response,)> {
    warp::any()
        .map(|| {
            let payload = json!({
                "code": 404,
                "name": "ControllerDisabled",
                "message": "All REST controllers are disabled in RestServerSettings.",
            });
            let mut response =
                warp::reply::with_status(warp::reply::json(&payload), StatusCode::NOT_FOUND)
                    .into_response();
            RestServerMiddleware::set_server_information_header(&mut response);
            response
        })
        .boxed()
}

fn socket_addr(address: IpAddr, port: u16) -> SocketAddr {
    SocketAddr::new(address, port)
}

fn log_join_error(error: JoinError) {
    if error.is_cancelled() {
        return;
    }
    warn!("REST server task finished with error: {}", error);
}

fn combine_filters(filters: Vec<BoxedFilter<(Response,)>>) -> BoxedFilter<(Response,)> {
    let mut iter = filters.into_iter();
    let first = iter.next().expect("combine_filters requires at least one route");
    iter.fold(first, |acc, filter| acc.or(filter).unify().boxed())
}

#[derive(Debug)]
struct Unauthorized;

impl Reject for Unauthorized {}

async fn handle_rejection(err: warp::Rejection) -> Result<Response, Infallible> {
    if err.find::<Unauthorized>().is_some() {
        let mut response = warp::reply::with_status(
            warp::reply::json(&ErrorModel::with_params(
                StatusCode::UNAUTHORIZED.as_u16() as i32,
                "AuthenticationFailed".to_string(),
                "Authentication Failed!".to_string(),
            )),
            StatusCode::UNAUTHORIZED,
        )
        .into_response();
        response.headers_mut().insert(
            WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"neo\""),
        );
        RestServerMiddleware::set_server_information_header(&mut response);
        return Ok(response);
    }

    let mut response = warp::reply::with_status(
        warp::reply::json(&ErrorModel::with_params(
            StatusCode::INTERNAL_SERVER_ERROR.as_u16() as i32,
            "UnhandledRejection".to_string(),
            "Request could not be processed".to_string(),
        )),
        StatusCode::INTERNAL_SERVER_ERROR,
    )
    .into_response();
    RestServerMiddleware::set_server_information_header(&mut response);
    Ok(response)
}

#[derive(Debug, Default, Deserialize)]
struct InvokeQuery {
    #[serde(default)]
    method: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct PaginationQuery {
    #[serde(default)]
    page: Option<i32>,
    #[serde(default)]
    size: Option<i32>,
}
