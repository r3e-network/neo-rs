//! Opened node runtime and requested-mode execution.

use crate::node::cli::NodeCli;
use crate::node::config::NodeConfig;
use crate::node::logging::LoggingGuards;
use crate::node::observability::ObservabilityRuntime;
use crate::node::workflow::RunningNode;

/// Result of opening runtime resources for an operator command.
pub(in crate::node) enum OpenNodeRuntime {
    /// A check-only command completed during preflight.
    Exit,
    /// The node is composed and ready to execute its requested mode.
    Ready(NodeRuntime),
}

impl OpenNodeRuntime {
    /// Execute the requested import/live-node mode, then stop gracefully.
    pub(in crate::node) async fn run_requested_mode(self) -> anyhow::Result<()> {
        match self {
            Self::Exit => Ok(()),
            Self::Ready(runtime) => runtime.run_requested_mode().await,
        }
    }
}

/// Fully opened daemon runtime.
pub(in crate::node) struct NodeRuntime {
    cli: NodeCli,
    config: NodeConfig,
    network_magic: u32,
    _logging_guards: LoggingGuards,
    observability: Option<ObservabilityRuntime>,
    running_node: RunningNode,
}

impl NodeRuntime {
    pub(super) fn new(
        cli: NodeCli,
        config: NodeConfig,
        network_magic: u32,
        logging_guards: LoggingGuards,
        observability: Option<ObservabilityRuntime>,
        running_node: RunningNode,
    ) -> Self {
        Self {
            cli,
            config,
            network_magic,
            _logging_guards: logging_guards,
            observability,
            running_node,
        }
    }

    async fn run_requested_mode(self) -> anyhow::Result<()> {
        let Self {
            cli,
            config,
            network_magic,
            _logging_guards,
            observability,
            running_node,
        } = self;
        running_node
            .run_requested_mode(&cli, &config, network_magic, observability.as_ref())
            .await
    }
}
