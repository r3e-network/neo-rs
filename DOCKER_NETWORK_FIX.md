# Docker Network Connection Fix

## Problem
Docker cannot connect to Docker Hub (registry-1.docker.io) even though:
1. Docker Desktop is running
2. Internet connection works (proxy at 127.0.0.1:7890 is functional)
3. Docker commands work locally

## Root Causes
1. **Proxy Configuration**: Your system uses a proxy (127.0.0.1:7890) but Docker might not be configured correctly
2. **Docker Desktop Network**: Docker Desktop on macOS runs in a VM that may have different network settings
3. **DNS Issues**: Docker might not resolve registry addresses correctly

## Solutions

### Solution 1: Configure Docker Desktop Proxy Settings

1. Open Docker Desktop
2. Go to Settings (gear icon) → Resources → Proxies
3. Enable "Manual proxy configuration"
4. Set:
   - Web Server (HTTP): `http://127.0.0.1:7890`
   - Secure Web Server (HTTPS): `http://127.0.0.1:7890`
   - Bypass proxy settings for: `localhost,127.0.0.1`
5. Click "Apply & restart"

### Solution 2: Use Docker with System Proxy

```bash
# Set proxy for Docker daemon
cat > ~/.docker/daemon.json <<EOF
{
  "proxies": {
    "http-proxy": "http://127.0.0.1:7890",
    "https-proxy": "http://127.0.0.1:7890",
    "no-proxy": "localhost,127.0.0.1"
  }
}
EOF

# Restart Docker Desktop
osascript -e 'quit app "Docker"'
sleep 10
open -a Docker
```

### Solution 3: Use Alternative Registry (China Mirrors)

Since you have a proxy at port 7890 (commonly used in China), try using Chinese Docker mirrors:

```bash
# Configure mirrors in Docker Desktop
cat > ~/.docker/daemon.json <<EOF
{
  "registry-mirrors": [
    "https://docker.mirrors.ustc.edu.cn",
    "https://hub-mirror.c.163.com",
    "https://registry.docker-cn.com"
  ]
}
EOF
```

### Solution 4: Build Using Proxy Environment

```bash
# Build with proxy environment variables
export HTTP_PROXY=http://127.0.0.1:7890
export HTTPS_PROXY=http://127.0.0.1:7890
export NO_PROXY=localhost,127.0.0.1

docker build \
  --build-arg HTTP_PROXY=$HTTP_PROXY \
  --build-arg HTTPS_PROXY=$HTTPS_PROXY \
  --build-arg NO_PROXY=$NO_PROXY \
  -t neo-rs:testnet .
```

### Solution 5: Download Images Manually

If Docker Hub is blocked, you can:

1. Use a VPN or different network
2. Download images on another machine and transfer them:
   ```bash
   # On a machine with access:
   docker pull rust:1.75-bullseye
   docker save rust:1.75-bullseye > rust-image.tar
   
   # On your machine:
   docker load < rust-image.tar
   ```

### Solution 6: Use Pre-built Binary (Recommended)

Since the Neo-RS binary is already built, run it without Docker:

```bash
# The node is already running on your system
./scripts/neo-node-manager.sh status

# To restart if needed:
./scripts/neo-node-manager.sh restart
```

## Current Status

- ✅ Neo-RS binary is built and working
- ✅ Node is running on ports 30332 (RPC) and 30333 (P2P)
- ❌ Docker cannot pull images due to network issues
- ✅ All Docker configuration files are ready for when network works

## Quick Test

Test if Docker can reach registries:

```bash
# Test connectivity
curl -x http://127.0.0.1:7890 https://registry-1.docker.io/v2/

# Test Docker pull with proxy
docker pull --platform linux/amd64 alpine:latest
```

## Recommendation

Continue using the Neo-RS node without Docker production ready. The binary is working perfectly:
- RPC endpoint: http://localhost:30332/rpc
- Management: `./scripts/neo-node-manager.sh`

When network issues are resolved, the Docker setup is ready to use.