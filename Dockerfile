# Multi-stage Dockerfile for Neo Rust Node
# R3E Network <jimmy@r3e.network>

FROM rust:1.89-bullseye AS builder

# Install system dependencies for building
RUN apt-get update && apt-get install -y \
    build-essential \
    gcc \
    g++ \
    cmake \
    make \
    pkg-config \
    llvm \
    libclang-dev \
    clang \
    libsnappy-dev \
    liblz4-dev \
    libzstd-dev \
    zlib1g-dev \
    libbz2-dev \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# libclang for bindgen (MDBX bindings). Bullseye ships LLVM 11; the
# libclang-dev package puts libclang.so under /usr/lib/llvm-11/lib. The ENV
# must be set directly (not via bashrc) so it's visible to the RUN cargo build.
ENV LIBCLANG_PATH=/usr/lib/llvm-11/lib

WORKDIR /workspace/neo-rs

# Copy manifests and workspace crates (kept explicit for better Docker layer caching).
COPY Cargo.toml Cargo.lock ./
COPY neo-primitives/ neo-primitives/
COPY neo-config/ neo-config/
COPY neo-crypto/ neo-crypto/
COPY neo-storage/ neo-storage/
COPY neo-static-files/ neo-static-files/
COPY neo-io/ neo-io/
COPY neo-vm/ neo-vm/
COPY neo-error/ neo-error/
COPY neo-serialization/ neo-serialization/
COPY neo-manifest/ neo-manifest/
COPY neo-payloads/ neo-payloads/
COPY neo-consensus/ neo-consensus/
COPY neo-hsm/ neo-hsm/
COPY neo-runtime/ neo-runtime/
COPY neo-execution/ neo-execution/
COPY neo-native-contracts/ neo-native-contracts/
COPY neo-state-service/ neo-state-service/
COPY neo-mempool/ neo-mempool/
COPY neo-blockchain/ neo-blockchain/
COPY neo-network/ neo-network/
COPY neo-wallets/ neo-wallets/
COPY neo-indexer/ neo-indexer/
COPY neo-system/ neo-system/
COPY neo-rpc/ neo-rpc/
COPY neo-oracle-service/ neo-oracle-service/
COPY neo-node/ neo-node/
COPY neo-test-fixtures/ neo-test-fixtures/
COPY tests/ tests/
COPY benches-package/ benches-package/
COPY scripts/ scripts/
COPY neo_mainnet_node.toml neo_testnet_node.toml neo_production_node.toml ./

# Build the node daemon.
RUN cargo build --release --locked -p neo-node

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
    unzip \
    && rm -rf /var/lib/apt/lists/*

# Create neo user and home
RUN groupadd -r neo && useradd -r -g neo -d /home/neo neo \
    && mkdir -p /home/neo && chown -R neo:neo /home/neo

# Create data directories (Logs for default config; keep /data/logs for backward compatibility)
RUN mkdir -p /data /data/blocks /data/Logs /data/logs && chown -R neo:neo /data

# Copy binaries from builder stage
COPY --from=builder /workspace/neo-rs/target/release/neo-node /usr/local/bin/neo-node

# Copy default configs and entrypoint
COPY neo_mainnet_node.toml /etc/neo/neo_mainnet_node.toml
COPY neo_testnet_node.toml /etc/neo/neo_testnet_node.toml
COPY neo_production_node.toml /etc/neo/neo_production_node.toml
COPY config/*.toml /etc/neo/config/
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
# Telemetry / health endpoints used by service-provider presets
EXPOSE 9090 9091

# Health check - JSON-RPC getversion on the configured RPC port
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD ["sh", "-c", "port_file=/tmp/neo_rpc_port; if [ -f \"$port_file\" ]; then port=$(cat \"$port_file\"); else port=${NEO_RPC_PORT:-}; fi; if [ -z \"$port\" ]; then port=20332; case \"${NEO_NETWORK:-testnet}\" in [Mm]ain*) port=10332 ;; esac; fi; curl -sf -X POST -H 'Content-Type: application/json' --data '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getversion\",\"params\":[]}' http://127.0.0.1:${port} >/dev/null"]

# Environment variables
ENV NEO_NETWORK=testnet \
    NEO_BACKEND=mdbx \
    NEO_PLUGINS_DIR=/data/Plugins \
    RUST_LOG=info

# Default command for neo-cli (configurable via env)
ENTRYPOINT ["/usr/local/bin/neo-entrypoint.sh"]
CMD []

# Metadata
LABEL org.opencontainers.image.title="Neo-Rust-Node"
LABEL org.opencontainers.image.description="Rust implementation of the Neo N3 blockchain protocol"
LABEL org.opencontainers.image.url="https://github.com/r3e-network/neo-rs"
LABEL org.opencontainers.image.documentation="https://github.com/r3e-network/neo-rs/blob/master/README.md"
LABEL org.opencontainers.image.source="https://github.com/r3e-network/neo-rs"
LABEL org.opencontainers.image.vendor="R3E Network"
LABEL org.opencontainers.image.licenses="MIT"
