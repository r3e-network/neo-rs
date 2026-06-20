//! External node observability hooks.

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use futures::FutureExt;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use self::config_validation::validate_runtime_config;
use self::endpoints::{
    apply_async_auth_and_headers, apply_blocking_auth_and_headers, error_endpoint_name,
    error_endpoint_url, heartbeat_endpoint_name, heartbeat_endpoint_url, normalized_kind, trimmed,
    trimmed_or_default,
};
use self::health::node_health_payload;
use self::payloads::{
    ErrorReport, ObservabilityMetadata, build_better_stack_error_payload,
    build_generic_error_payload, build_google_error_payload, build_heartbeat_payload,
    build_sentry_error_payload,
};
use super::config::{
    ObservabilityErrorEndpoint, ObservabilityHeartbeatEndpoint, ObservabilitySection,
};

mod config_validation;
mod endpoints;
mod health;
mod payloads;

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) struct ObservabilityRuntime {
    inner: Arc<ObservabilityInner>,
}

struct ObservabilityInner {
    config: ObservabilitySection,
    metadata: ObservabilityMetadata,
    async_client: reqwest::Client,
}

impl ObservabilityRuntime {
    pub(super) fn from_config(
        config: &ObservabilitySection,
        network: u32,
    ) -> anyhow::Result<Option<Self>> {
        if !config.enabled {
            return Ok(None);
        }
        validate_runtime_config(config)?;

        let timeout = Duration::from_millis(config.request_timeout_ms);
        let async_client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .context("building async observability HTTP client")?;

        let runtime = Self {
            inner: Arc::new(ObservabilityInner {
                config: config.clone(),
                metadata: ObservabilityMetadata {
                    service_name: trimmed_or_default(config.service_name.as_deref(), "neo-node"),
                    environment: trimmed(config.environment.as_deref()),
                    node_id: trimmed(config.node_id.as_deref()),
                    network,
                    version: env!("CARGO_PKG_VERSION"),
                },
                async_client,
            }),
        };

        info!(
            target: "neo::observability",
            error_endpoints = runtime
                .inner
                .config
                .error_endpoints
                .iter()
                .filter(|endpoint| endpoint.enabled)
                .count(),
            heartbeat_endpoints = runtime
                .inner
                .config
                .heartbeat_endpoints
                .iter()
                .filter(|endpoint| endpoint.enabled)
                .count(),
            "observability enabled"
        );
        Ok(Some(runtime))
    }

    pub(super) fn install_panic_hook(&self) {
        if !self.inner.config.capture_panics {
            return;
        }

        let previous_hook = std::panic::take_hook();
        let inner = Arc::clone(&self.inner);
        std::panic::set_hook(Box::new(move |panic_info| {
            previous_hook(panic_info);
            let report = ErrorReport::from_panic(panic_info);
            if let Err(err) = inner.report_error_blocking(&report) {
                eprintln!("neo-node observability panic report failed: {err}");
            }
        }));
    }

    pub(super) fn report_startup_error(&self, error: &anyhow::Error) {
        let report = ErrorReport::startup(error);
        if let Err(err) = self.inner.report_error_blocking(&report) {
            warn!(
                target: "neo::observability",
                error = %err,
                "failed to report startup error"
            );
        }
    }

    pub(super) fn report_runtime_error(&self, component: &str, error: impl std::fmt::Display) {
        let report = ErrorReport::runtime_error(component, error);
        if let Err(err) = self.inner.report_error_blocking(&report) {
            warn!(
                target: "neo::observability",
                component,
                error = %err,
                "failed to report runtime error"
            );
        }
    }

    pub(super) fn spawn_monitored<F>(&self, task_name: &'static str, future: F) -> JoinHandle<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            let report = match AssertUnwindSafe(future).catch_unwind().await {
                Ok(()) => {
                    warn!(
                        target: "neo::observability",
                        task = task_name,
                        "background task exited unexpectedly"
                    );
                    ErrorReport::background_task_exit(task_name)
                }
                Err(payload) => {
                    warn!(
                        target: "neo::observability",
                        task = task_name,
                        "background task panicked"
                    );
                    ErrorReport::background_task_panic(task_name, payload.as_ref())
                }
            };
            if let Err(err) = inner.report_error_async(&report).await {
                warn!(
                    target: "neo::observability",
                    task = task_name,
                    error = %err,
                    "failed to report background task failure"
                );
            }
        })
    }

    pub(super) fn spawn_monitored_result<F>(
        &self,
        task_name: &'static str,
        future: F,
    ) -> JoinHandle<()>
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            let report = match AssertUnwindSafe(future).catch_unwind().await {
                Ok(Ok(())) => {
                    warn!(
                        target: "neo::observability",
                        task = task_name,
                        "background task exited unexpectedly"
                    );
                    ErrorReport::background_task_exit(task_name)
                }
                Ok(Err(err)) => {
                    warn!(
                        target: "neo::observability",
                        task = task_name,
                        error = %err,
                        "background task failed"
                    );
                    ErrorReport::background_task_error(task_name, &err)
                }
                Err(payload) => {
                    warn!(
                        target: "neo::observability",
                        task = task_name,
                        "background task panicked"
                    );
                    ErrorReport::background_task_panic(task_name, payload.as_ref())
                }
            };
            if let Err(err) = inner.report_error_async(&report).await {
                warn!(
                    target: "neo::observability",
                    task = task_name,
                    error = %err,
                    "failed to report background task failure"
                );
            }
        })
    }

    pub(super) fn spawn_heartbeat_tasks(&self, node: Arc<neo_system::Node>) -> Vec<JoinHandle<()>> {
        self.inner
            .config
            .heartbeat_endpoints
            .iter()
            .filter(|endpoint| endpoint.enabled)
            .cloned()
            .map(|endpoint| {
                let inner = Arc::clone(&self.inner);
                let node = Arc::clone(&node);
                let runtime = self.clone();
                runtime.spawn_monitored("observability_heartbeat", async move {
                    let interval_seconds = endpoint
                        .interval_seconds
                        .unwrap_or(inner.config.heartbeat_interval_seconds);
                    let mut interval = tokio::time::interval(Duration::from_secs(interval_seconds));
                    loop {
                        interval.tick().await;
                        if let Err(err) = inner.send_heartbeat_async(&endpoint, &node).await {
                            let endpoint_name = heartbeat_endpoint_name(&endpoint);
                            warn!(
                                target: "neo::observability",
                                endpoint = %endpoint_name,
                                error = %err,
                                "heartbeat request failed"
                            );
                            let report = ErrorReport::heartbeat_failure(&endpoint_name, &err);
                            if let Err(report_err) = inner.report_error_async(&report).await {
                                warn!(
                                    target: "neo::observability",
                                    endpoint = %endpoint_name,
                                    error = %report_err,
                                    "failed to report heartbeat failure"
                                );
                            }
                        }
                    }
                })
            })
            .collect()
    }
}

impl ObservabilityInner {
    fn report_error_blocking(&self, report: &ErrorReport) -> anyhow::Result<()> {
        let config = self.config.clone();
        let metadata = self.metadata.clone();
        let report = report.clone();
        let reporter = std::thread::Builder::new()
            .name("neo-observability-error-report".to_string())
            .spawn(move || report_error_blocking_on_thread(config, metadata, report))
            .context("spawning blocking observability reporter")?;

        match reporter.join() {
            Ok(result) => result,
            Err(_) => anyhow::bail!("blocking observability reporter panicked"),
        }
    }

    async fn report_error_async(&self, report: &ErrorReport) -> anyhow::Result<()> {
        let mut failures = Vec::new();
        for endpoint in self
            .config
            .error_endpoints
            .iter()
            .filter(|endpoint| endpoint.enabled)
        {
            if let Err(err) = self.send_error_async(endpoint, report).await {
                failures.push(format!("{}: {err}", error_endpoint_name(endpoint)));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(failures.join("; "))
        }
    }

    async fn send_error_async(
        &self,
        endpoint: &ObservabilityErrorEndpoint,
        report: &ErrorReport,
    ) -> anyhow::Result<()> {
        let kind = normalized_kind(endpoint.kind.as_deref());
        let url = error_endpoint_url(endpoint, &kind)?;
        let payload = match kind.as_str() {
            "google_error_reporting" => build_google_error_payload(&self.metadata, report),
            "better_stack_logs" => build_better_stack_error_payload(&self.metadata, report),
            "sentry" => build_sentry_error_payload(&self.metadata, report),
            _ => build_generic_error_payload(&self.metadata, report),
        };
        let request = self.async_client.post(&url).json(&payload);
        let request = apply_async_auth_and_headers(
            request,
            endpoint.token.as_deref(),
            endpoint.token_env.as_deref(),
            &endpoint.headers,
            &endpoint.headers_env,
        )?;
        let response = request
            .send()
            .await
            .with_context(|| format!("posting error report to {url}"))?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("error report endpoint returned HTTP {status}");
        }
        Ok(())
    }

    async fn send_heartbeat_async(
        &self,
        endpoint: &ObservabilityHeartbeatEndpoint,
        node: &neo_system::Node,
    ) -> anyhow::Result<()> {
        let url = heartbeat_endpoint_url(endpoint)?;
        let method = endpoint
            .method
            .as_deref()
            .unwrap_or("GET")
            .to_ascii_uppercase();
        let request = match method.as_str() {
            "GET" => self.async_client.get(url),
            "POST" => {
                let payload =
                    build_heartbeat_payload(&self.metadata, Some(node_health_payload(node)));
                self.async_client.post(url).json(&payload)
            }
            "PUT" => {
                let payload =
                    build_heartbeat_payload(&self.metadata, Some(node_health_payload(node)));
                self.async_client.put(url).json(&payload)
            }
            _ => anyhow::bail!("unsupported heartbeat method {method:?}"),
        };
        let request = apply_async_auth_and_headers(
            request,
            endpoint.token.as_deref(),
            endpoint.token_env.as_deref(),
            &endpoint.headers,
            &endpoint.headers_env,
        )?;
        let response = request
            .send()
            .await
            .with_context(|| format!("sending heartbeat to {url}"))?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("heartbeat endpoint returned HTTP {status}");
        }
        Ok(())
    }
}

fn report_error_blocking_on_thread(
    config: ObservabilitySection,
    metadata: ObservabilityMetadata,
    report: ErrorReport,
) -> anyhow::Result<()> {
    let timeout = Duration::from_millis(config.request_timeout_ms);
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .context("building blocking observability HTTP client")?;

    report_error_with_blocking_client(&client, &config, &metadata, &report)
}

fn report_error_with_blocking_client(
    client: &reqwest::blocking::Client,
    config: &ObservabilitySection,
    metadata: &ObservabilityMetadata,
    report: &ErrorReport,
) -> anyhow::Result<()> {
    let mut failures = Vec::new();
    for endpoint in config
        .error_endpoints
        .iter()
        .filter(|endpoint| endpoint.enabled)
    {
        if let Err(err) = send_error_blocking(client, metadata, endpoint, report) {
            failures.push(format!("{}: {err}", error_endpoint_name(endpoint)));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(failures.join("; "))
    }
}

fn send_error_blocking(
    client: &reqwest::blocking::Client,
    metadata: &ObservabilityMetadata,
    endpoint: &ObservabilityErrorEndpoint,
    report: &ErrorReport,
) -> anyhow::Result<()> {
    let kind = normalized_kind(endpoint.kind.as_deref());
    let url = error_endpoint_url(endpoint, &kind)?;
    let payload = match kind.as_str() {
        "google_error_reporting" => build_google_error_payload(metadata, report),
        "better_stack_logs" => build_better_stack_error_payload(metadata, report),
        "sentry" => build_sentry_error_payload(metadata, report),
        _ => build_generic_error_payload(metadata, report),
    };
    let request = client.post(&url).json(&payload);
    let request = apply_blocking_auth_and_headers(
        request,
        endpoint.token.as_deref(),
        endpoint.token_env.as_deref(),
        &endpoint.headers,
        &endpoint.headers_env,
    )?;
    let response = request
        .send()
        .with_context(|| format!("posting error report to {url}"))?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("error report endpoint returned HTTP {status}");
    }
    Ok(())
}
