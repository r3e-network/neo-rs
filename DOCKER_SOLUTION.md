# Docker Build Solution for Neo-RS

## Current Situation

Docker cannot pull images from any registry (Docker Hub or Chinese mirrors) due to network timeouts. This is likely due to:
1. Firewall/network restrictions
2. Proxy configuration issues in Docker Desktop
3. SSL/TLS handshake timeouts

## Immediate Solution: Run Without Docker

The Neo-RS node is already built and running successfully:

```bash
# Check current status
./scripts/neo-node-manager.sh status

# The node is running on:
# - RPC: http://localhost:30332/rpc
# - P2P: 30333
```

## Docker Solutions to Try

### 1. Configure Docker Desktop Proxy (GUI Method)

1. Open Docker Desktop
2. Click the gear icon (Settings)
3. Go to "Resources" → "Proxies"
4. Enable "Manual proxy configuration"
5. Set:
   - Web Server (HTTP): `http://127.0.0.1:7890`
   - Secure Web Server (HTTPS): `http://127.0.0.1:7890`
   - Bypass: `localhost,127.0.0.1,*.local`
6. Click "Apply & restart"

### 2. Use Different Registry (阿里云)

If you have an Aliyun account:
1. Login to https://cr.console.aliyun.com
2. Get your personal accelerator address
3. Update ~/.docker/daemon.json:
```json
{
  "registry-mirrors": ["https://<your-id>.mirror.aliyuncs.com"]
}
```

### 3. Manual Image Import

If someone can provide you with the Docker images:

```bash
# On a machine with access:
docker pull rust:1.75-bullseye
docker pull debian:bullseye-slim
docker save rust:1.75-bullseye debian:bullseye-slim -o neo-base-images.tar

# On your machine:
docker load -i neo-base-images.tar
docker build -t neo-rs:testnet .
```

### 4. Use a VPN

Temporarily use a VPN to bypass network restrictions:
1. Connect to VPN
2. Restart Docker Desktop
3. Run: `docker build -t neo-rs:testnet .`

## Current Working Setup

While Docker issues are being resolved, the Neo-RS node is fully functional:

### Running Node Info:
- Binary: `./target/release/neo-node`
- Config: TestNet
- RPC Port: 30332
- P2P Port: 30333
- Management: `./scripts/neo-node-manager.sh`

### Available Commands:
```bash
# Check status
./scripts/neo-node-manager.sh status

# View logs
./scripts/neo-node-manager.sh logs

# Test RPC
curl -X POST http://localhost:30332/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}'

# Stop node
./scripts/neo-node-manager.sh stop

# Start node
./scripts/neo-node-manager.sh start
```

## When Docker Works

All Docker files are ready:
- `Dockerfile` - Full multi-stage build
- `docker-compose.testnet.yml` - Easy deployment
- `DOCKER_INSTRUCTIONS.md` - Complete guide

Just run:
```bash
docker build -t neo-rs:testnet .
docker-compose -f docker-compose.testnet.yml up -d
```

## Recommendation

Continue using the native binary (`./scripts/neo-node-manager.sh`) until Docker network issues are resolved. The node is fully functional and performing well.