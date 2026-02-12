# Multi-stage Dockerfile for Neo Rust Node
# R3E Network <jimmy@r3e.network>

FROM rust:1.76-bullseye as builder

# Install system dependencies for building
RUN apt-get update && apt-get install -y \
    build-essential \
    gcc \
    g++ \
    cmake \
    make \
    pkg-config \
    llvm-14 \
    libclang-14-dev \
    clang-14 \
    libsnappy-dev \
    liblz4-dev \
    libzstd-dev \
    zlib1g-dev \
    libbz2-dev \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Set environment variables for libclang
ENV LIBCLANG_PATH=/usr/lib/llvm-14/lib

# Create app directory
WORKDIR /app

# Copy manifests and workspace crates (kept explicit for better Docker layer caching).
COPY Cargo.toml Cargo.lock ./
COPY neo-primitives/ neo-primitives/
COPY neo-crypto/ neo-crypto/
COPY neo-storage/ neo-storage/
COPY neo-io/ neo-io/
COPY neo-json/ neo-json/
COPY neo-core/ neo-core/
COPY neo-vm/ neo-vm/
COPY neo-p2p/ neo-p2p/
COPY neo-rpc/ neo-rpc/
COPY neo-consensus/ neo-consensus/
COPY neo-tee/ neo-tee/
COPY neo-hsm/ neo-hsm/
COPY neo-telemetry/ neo-telemetry/
COPY neo-cli/ neo-cli/
COPY neo-node/ neo-node/
COPY scripts/ scripts/
COPY neo_mainnet_node.toml neo_testnet_node.toml neo_production_node.toml ./

# Build release binaries (neo-node daemon + neo-cli client)
RUN cargo build --release --locked -p neo-node -p neo-cli

# Runtime stage
FROM debian:bullseye-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    bash \
    libsnappy1v5 \
    liblz4-1 \
    libzstd1 \
    zlib1g \
    libbz2-1.0 \
    libssl1.1 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create neo user and home
RUN groupadd -r neo && useradd -r -g neo -d /home/neo neo \
    && mkdir -p /home/neo && chown -R neo:neo /home/neo

# Create data directories (Logs for default config; keep /data/logs for backward compatibility)
RUN mkdir -p /data /data/blocks /data/Logs /data/logs && chown -R neo:neo /data

# Copy binaries from builder stage
COPY --from=builder /app/target/release/neo-node /usr/local/bin/neo-node
COPY --from=builder /app/target/release/neo-cli /usr/local/bin/neo-cli

# Copy default configs and entrypoint
COPY neo_mainnet_node.toml /etc/neo/neo_mainnet_node.toml
COPY neo_testnet_node.toml /etc/neo/neo_testnet_node.toml
COPY neo_production_node.toml /etc/neo/neo_production_node.toml
COPY scripts/docker-entrypoint.sh /usr/local/bin/neo-entrypoint.sh
RUN chmod +x /usr/local/bin/neo-entrypoint.sh && chown -R neo:neo /etc/neo

# Set up volumes
VOLUME ["/data"]

# Switch to neo user and working directory
USER neo
WORKDIR /data
ENV HOME=/home/neo

# Expose ports
# TestNet ports
EXPOSE 20332 20333
# MainNet ports
EXPOSE 10332 10333
# Private network ports
EXPOSE 30332 30333

# Health check - JSON-RPC getversion on the configured RPC port
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD ["sh", "-c", "port_file=/tmp/neo_rpc_port; if [ -f \"$port_file\" ]; then port=$(cat \"$port_file\"); else port=${NEO_RPC_PORT:-}; fi; if [ -z \"$port\" ]; then port=20332; case \"${NEO_NETWORK:-testnet}\" in [Mm]ain*) port=10332 ;; esac; fi; curl -sf -X POST -H 'Content-Type: application/json' --data '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getversion\",\"params\":[]}' http://127.0.0.1:${port} >/dev/null"]

# Environment variables
ENV NEO_NETWORK=testnet \
    NEO_BACKEND=rocksdb \
    NEO_PLUGINS_DIR=/data/Plugins \
    RUST_LOG=info

# Default command for neo-cli (configurable via env)
ENTRYPOINT ["/usr/local/bin/neo-entrypoint.sh"]
CMD []

# Metadata
LABEL org.opencontainers.image.title="Neo-Rust-Node"
LABEL org.opencontainers.image.description="Production-ready Rust implementation of the Neo N3 blockchain protocol"
LABEL org.opencontainers.image.url="https://github.com/r3e-network/neo-rs"
LABEL org.opencontainers.image.documentation="https://github.com/r3e-network/neo-rs/blob/master/README.md"
LABEL org.opencontainers.image.source="https://github.com/r3e-network/neo-rs"
LABEL org.opencontainers.image.vendor="R3E Network"
LABEL org.opencontainers.image.licenses="MIT"
