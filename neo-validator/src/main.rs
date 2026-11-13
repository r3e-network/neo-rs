use anyhow::{Context, Result};
use clap::Parser;
use neo_validator::audit::AuditLog;
use neo_validator::config::{TokenDigest, ValidatorConfig};
use neo_validator::discovery::discover_protocol;
use neo_validator::executor::{BlockExecutor, NeoExecutor};
use neo_validator::gateway::{AdminApi, ProxyGateway};
use neo_validator::state::GateRegistry;
use neo_validator::synchronizer::Synchronizer;
use neo_validator::upstream::{HttpNode, NodeKind, NodeRpc};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(
    name = "neo-validator",
    about = "Fail-closed neo-cli / neo-go online state validator"
)]
struct Cli {
    #[arg(long, default_value = "neo-validator.toml")]
    config: PathBuf,
    #[arg(long)]
    check_config: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = ValidatorConfig::load(&cli.config)?;
    let (settings, protocol_path) = discover_protocol(&config)?;
    if cli.check_config {
        println!(
            "configuration valid: network={}, protocol={}",
            settings.network,
            protocol_path.display()
        );
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let timeout = Duration::from_millis(config.validator.rpc_timeout_ms);
    let cli_http = HttpNode::new(
        NodeKind::NeoCli,
        &config.neo_cli,
        timeout,
        config.security.max_response_body_bytes,
        config.validator.max_block_bytes,
    )?;
    let go_http = HttpNode::new(
        NodeKind::NeoGo,
        &config.neo_go,
        timeout,
        config.security.max_response_body_bytes,
        config.validator.max_block_bytes,
    )?;
    let executor = Arc::new(NeoExecutor::open(
        settings.clone(),
        &config.validator.data_path,
    )?);
    let execution_mode = config
        .validator
        .execution_mode
        .parse()
        .map_err(anyhow::Error::msg)?;
    executor.set_execution_mode(execution_mode);
    let gates = GateRegistry::default();
    let audit = AuditLog::open(&config.validator.audit_path)?;
    let cli_rpc: Arc<dyn NodeRpc> = cli_http.clone();
    let go_rpc: Arc<dyn NodeRpc> = go_http.clone();
    let block_executor: Arc<dyn BlockExecutor> = executor.clone();
    let synchronizer = Arc::new(Synchronizer::new(
        cli_rpc,
        go_rpc,
        block_executor.clone(),
        gates.clone(),
        audit.clone(),
        settings,
        Duration::from_millis(config.validator.poll_interval_ms),
        config.validator.max_catch_up_blocks,
    ));
    synchronizer
        .initialize()
        .await
        .context("validator initialization failed")?;

    let allowed_methods = Arc::new(config.security.allowed_methods.clone());
    let proxy_token = config
        .security
        .proxy_token_sha256
        .as_deref()
        .map(|value| TokenDigest::parse(value, "proxy_token_sha256"))
        .transpose()?;
    let cli_gateway = ProxyGateway::new(
        cli_http,
        gates.clone(),
        allowed_methods.clone(),
        proxy_token.clone(),
        config.security.max_request_body_bytes,
    );
    let go_gateway = ProxyGateway::new(
        go_http,
        gates.clone(),
        allowed_methods,
        proxy_token,
        config.security.max_request_body_bytes,
    );
    let admin = AdminApi::new(
        gates,
        block_executor,
        audit,
        TokenDigest::parse(&config.security.read_token_sha256, "read_token_sha256")?,
        TokenDigest::parse(&config.security.admin_token_sha256, "admin_token_sha256")?,
        config.security.max_request_body_bytes,
    );

    let cli_listen = config.neo_cli.proxy_listen.unwrap();
    let go_listen = config.neo_go.proxy_listen.unwrap();
    let admin_listen = config.security.admin_listen;
    tracing::info!(
        target: "neo_validator",
        %cli_listen,
        %go_listen,
        %admin_listen,
        "validator and fail-closed gateways started"
    );
    let sync_task = synchronizer.run();
    let cli_task = cli_gateway.serve(cli_listen);
    let go_task = go_gateway.serve(go_listen);
    let admin_task = admin.serve(admin_listen);
    tokio::pin!(sync_task, cli_task, go_task, admin_task);

    let failure = tokio::select! {
        _ = tokio::signal::ctrl_c() => None,
        result = &mut sync_task => Some(result.context("synchronizer stopped unexpectedly")),
        result = &mut cli_task => Some(result.context("neo-cli gateway stopped unexpectedly")),
        result = &mut go_task => Some(result.context("neo-go gateway stopped unexpectedly")),
        result = &mut admin_task => Some(result.context("admin API stopped unexpectedly")),
    };
    executor.shutdown().await?;
    if let Some(result) = failure {
        result?;
    }
    Ok(())
}
