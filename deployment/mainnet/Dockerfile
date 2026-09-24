FROM rust:1.88-bullseye as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    libssl-dev \
    librocksdb-dev \
    clang \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Create build directory
WORKDIR /build

# Copy source code
COPY . .

# Build production binary with full features
RUN cargo build --profile production --features "full" --package neo-node

# Runtime stage
FROM debian:bullseye-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    librocksdb6.11 \
    libssl1.1 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create neo user
RUN useradd --system --no-create-home --shell /usr/sbin/nologin neo

# Create directory structure
RUN mkdir -p /opt/neo-node/{bin,config,data,logs} && \
    chown -R neo:neo /opt/neo-node && \
    chmod 700 /opt/neo-node/data

# Copy binary from builder
COPY --from=builder /build/target/production/neo-node /opt/neo-node/bin/neo-node

# Copy configuration
COPY deployment/mainnet/config/mainnet-production.toml /opt/neo-node/config/

# Set permissions
RUN chmod 755 /opt/neo-node/bin/neo-node && \
    chmod 600 /opt/neo-node/config/mainnet-production.toml && \
    chown neo:neo /opt/neo-node/bin/neo-node /opt/neo-node/config/mainnet-production.toml

# Switch to neo user
USER neo
WORKDIR /opt/neo-node

# Expose ports
EXPOSE 10333 10332 9090

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD curl -f http://localhost:10332 -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}' || exit 1

# Start neo-node
ENTRYPOINT ["/opt/neo-node/bin/neo-node"]
CMD ["--config", "/opt/neo-node/config/mainnet-production.toml"]
