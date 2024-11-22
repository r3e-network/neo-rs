FROM mcr.microsoft.com/devcontainers/rust:latest

# Install additional tools and dependencies
RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
    && apt-get -y install --no-install-recommends \
    curl \
    git \
    && apt-get clean -y && rm -rf /var/lib/apt/lists/*

# Install additional Rust tools
RUN cargo install cargo-watch cargo-edit

# Set working directory
WORKDIR /workspace

# Set user
USER vscode

# Set proxy environment variables
ENV http_proxy=http://localhost:7890
ENV https_proxy=http://localhost:7890