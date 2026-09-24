# Neo-RS Mainnet Deployment Package - Build Instructions

## Current Status
⚠️ **Build Issue**: The production build encountered a RocksDB compilation error on Windows with the MinGW toolchain. This is a known issue with the `librocksdb-sys` crate on Windows.

## Build Solutions

### Option 1: Linux Build (RECOMMENDED)
Build on Linux or WSL2 for best results:

```bash
# On Ubuntu/Debian
sudo apt-get install -y build-essential libssl-dev librocksdb-dev clang
cargo build --profile production --features "full" --package neo-node

# Binary location
./target/production/neo-node
```

### Option 2: Windows MSVC Build
Use the MSVC toolchain instead of MinGW:

```bash
# Switch to MSVC toolchain
rustup default stable-x86_64-pc-windows-msvc

# Install Visual Studio Build Tools with C++ support
# Download from: https://visualstudio.microsoft.com/downloads/

# Build
cargo build --profile production --features "full" --package neo-node --target x86_64-pc-windows-msvc

# Binary location
.\target\x86_64-pc-windows-msvc\production\neo-node.exe
```

### Option 3: Docker Build (CROSS-PLATFORM)
Build inside a Docker container:

```bash
cd /d/Git/neo-rs
docker build -f deployment/mainnet/Dockerfile -t neo-node:mainnet .

# Extract binary from image
docker create --name neo-temp neo-node:mainnet
docker cp neo-temp:/opt/neo-node/bin/neo-node ./deployment/mainnet/bin/
docker rm neo-temp
```

### Option 4: GitHub Actions / CI Build
Set up automated builds in CI that run on Linux runners.

## Next Steps After Successful Build

1. Copy binary to deployment package:
   ```bash
   mkdir -p ./deployment/mainnet/bin
   cp ./target/production/neo-node ./deployment/mainnet/bin/
   ```

2. Create deployment tarball:
   ```bash
   cd deployment
   tar -czf neo-rs-mainnet-v0.17.0.tar.gz mainnet/
   ```

3. Deploy to production server:
   ```bash
   scp neo-rs-mainnet-v0.17.0.tar.gz user@server:/tmp/
   ssh user@server
   cd /tmp
   tar -xzf neo-rs-mainnet-v0.17.0.tar.gz
   cd mainnet
   sudo ./scripts/install.sh
   ```

## Build Error Details

The error encountered:
```
rocksdb/port/win/env_win.cc:920:3: error: unknown type name 'FILE_ID_INFO'
```

This is caused by:
- MinGW toolchain missing Windows SDK definitions
- RocksDB expecting newer Windows API headers
- Known issue: https://github.com/rust-rocksdb/rust-rocksdb/issues/

## Alternative: Pre-built Binaries

For production deployment, consider:
1. Build on a Linux CI server
2. Cross-compile from Linux to Windows (if needed)
3. Use official releases when available
4. Build in Docker and extract binary

## Verification

After building, verify the binary:

```bash
# Check it's stripped
file ./target/production/neo-node
# Should show: "stripped"

# Check size (should be reasonable, ~50-200MB)
ls -lh ./target/production/neo-node

# Run version check
./target/production/neo-node --version

# Test startup (will fail without config, which is expected)
./target/production/neo-node --help
```
