# Multi-stage build for Neo-RS
FROM rust:1.75-bullseye as builder

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    librocksdb-dev \
    clang \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY node/ node/

# Build release binary
RUN cargo build --release --bin neo-rs

# Runtime stage
FROM debian:bullseye-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    librocksdb6.20 \
    libssl1.1 \
    && rm -rf /var/lib/apt/lists/*

# Create neo user
RUN groupadd -r neo && useradd -r -g neo neo

# Create data directory
RUN mkdir -p /data && chown neo:neo /data

# Copy binary from builder stage
COPY --from=builder /app/target/release/neo-rs /usr/local/bin/neo-rs

# Copy configuration
COPY neo-config.toml /etc/neo/neo-config.toml

# Set up volumes
VOLUME ["/data"]

# Switch to neo user
USER neo

# Expose ports
EXPOSE 10333 10332

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD neo-rs --health-check || exit 1

# Default command
ENTRYPOINT ["neo-rs"]
CMD ["--config", "/etc/neo/neo-config.toml", "--data-dir", "/data"]

# Metadata
LABEL org.opencontainers.image.title="Neo-RS"
LABEL org.opencontainers.image.description="High-performance Rust implementation of the Neo N3 blockchain protocol"
LABEL org.opencontainers.image.url="https://github.com/neo-project/neo-rs"
LABEL org.opencontainers.image.documentation="https://docs.rs/neo-rs"
LABEL org.opencontainers.image.source="https://github.com/neo-project/neo-rs"
LABEL org.opencontainers.image.vendor="Neo Global Development"
LABEL org.opencontainers.image.licenses="MIT"