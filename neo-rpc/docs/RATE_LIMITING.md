# RPC Rate Limiting

## Overview

The Neo RPC server includes comprehensive rate limiting to protect against DoS attacks via request flooding. Rate limiting is enforced per-IP address with tiered limits based on operation complexity.

## Architecture

### Per-IP Rate Limiting
- Each client IP address has independent rate limit buckets
- Uses the `governor` crate with token bucket algorithm
- Automatic cleanup of stale IP entries (10 minute timeout)

### Tiered Rate Limits

Different RPC methods have different rate limit tiers based on their computational cost:

| Tier | Operations | Default Rate (req/s) | Default Burst |
|------|-----------|---------------------|---------------|
| **Cheap** | Simple state lookups | 200 | 400 |
| **Standard** | Normal operations | 100 | 200 |
| **Expensive** | VM execution, complex queries | 20 | 40 |
| **Write** | State-modifying operations | 10 | 20 |

#### Tier Assignments

**Cheap Operations** (2x standard rate):
- `getblockcount`
- `getconnectioncount`
- `getrawmempool`
- `getversion`
- `getcommittee`
- `getvalidators`
- `ping`

**Expensive Operations** (1/5 standard rate):
- `invokefunction`
- `invokescript`
- `invokecontractverify`
- `findstorage`
- `findstates`
- `traverseiterator`
- `terminatesession`
- `getunclaimedgas`
- `calculatenetworkfee`
- `getwalletunclaimedgas`

**Write Operations** (1/10 standard rate):
- `sendrawtransaction`
- `submitblock`

**Standard Operations** (base rate):
- All other RPC methods

## Configuration

### Basic Configuration

In your configuration file (e.g., `config/mainnet.toml`):

```toml
[rpc]
# Maximum sustained requests per second per IP
max_requests_per_second = 100

# Burst capacity for short spikes
rate_limit_burst = 200
```

### Disabling Rate Limiting

**⚠️ Not recommended for public nodes**

```toml
[rpc]
max_requests_per_second = 0  # Disables rate limiting
```

### Production Recommendations

**For Public RPC Nodes:**
```toml
max_requests_per_second = 100
rate_limit_burst = 200
auth_enabled = true  # Consider enabling authentication
```

**For Private/Internal Nodes:**
```toml
max_requests_per_second = 500  # Higher limits
rate_limit_burst = 1000
```

**For Development:**
```toml
max_requests_per_second = 0  # Can disable for convenience
```

## Behavior

### When Rate Limited

When a client exceeds their rate limit:
- HTTP Status: `200 OK` (per JSON-RPC 2.0 spec)
- JSON-RPC Error Code: `-32429`
- Error Message: `"Too many requests"`

Example response:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32429,
    "message": "Too many requests"
  }
}
```

### Rate Limit Enforcement Points

1. **Per-Request**: Each valid RPC request consumes tokens from the appropriate tier bucket
2. **Batch Requests**: Each call in a batch is rate-limited independently
3. **Invalid Requests**: Malformed requests are charged at the Standard tier to prevent bypass
4. **Pre-Method Resolution**: Rate limiting happens before method execution, minimizing resource waste

## Security Considerations

### Defense Against DoS

Rate limiting provides defense-in-depth against several attack vectors:

1. **Request Flooding**: Prevents overwhelming the server with legitimate requests
2. **Expensive Query Abuse**: Limits VM execution and complex queries (invokefunction, findstorage)
3. **Batch Amplification**: Combined with `max_batch_size` to prevent single-request amplification
4. **Write Spam**: Stricter limits on sendrawtransaction prevent mempool flooding

### Additional Protections

Rate limiting is one layer. Also configure:

```toml
[rpc]
max_batch_size = 1024           # Limit batch request size
max_request_body_size = 5242880 # 5MB limit
auth_enabled = true             # Basic authentication
disabled_methods = ["openwallet"] # Disable dangerous methods
bind_address = "127.0.0.1"     # Bind to localhost unless necessary
```

### Security Validation

On startup, the RPC server validates configuration:
- **Warning**: `max_requests_per_second = 0` on non-localhost bind addresses
- This catches accidental exposure of unprotected RPC servers

## Monitoring

### Metrics

Rate limiting is integrated with Prometheus metrics:

- `rpc_requests_total`: Total RPC requests (includes rate-limited)
- `rpc_errors_total`: Total errors (includes rate limit rejections)

### Logging

Rate limit events are logged at appropriate levels:
- Client rate limiting: Not logged per-request (would be noisy)
- Configuration warnings: Logged at `WARN` level on startup

## Testing

### Unit Tests

Rate limiting behavior is covered by comprehensive tests in `neo-rpc/tests/rate_limiter_governor.rs`:

```bash
cargo test --package neo-rpc --test rate_limiter_governor --features server
```

Tests cover:
- Burst exhaustion
- Per-IP bucket independence
- Tier-based limits
- Disabled mode behavior
- Configuration edge cases

### Integration Testing

To test rate limiting in a running node:

```bash
# Send rapid requests
for i in {1..150}; do
  curl -X POST http://localhost:10332 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}'
done
```

After ~100 requests, you should receive `-32429` errors.

## Implementation Details

### Code Organization

- **Middleware**: `neo-rpc/src/server/middleware/rate_limiter.rs`
  - `GovernorRateLimiter`: Main rate limiter implementation
  - `RateLimitConfig`: Configuration structure
  - `RateLimitTier`: Tier classification

- **Integration**: `neo-rpc/src/server/routes/handlers.rs`
  - `process_object`: Per-method rate limiting (lines 253-259)
  - `charge_standard_rate_limit`: Standard tier enforcement

- **Configuration**: `neo-rpc/src/server/rpc_server_settings.rs`
  - `RpcServerConfig`: Configuration deserialization
  - Default values and validation

### Token Bucket Algorithm

The implementation uses a token bucket algorithm via the `governor` crate:

1. Each IP has a bucket with `burst` tokens
2. Tokens refill at `max_rps` tokens per second
3. Each request consumes 1 token
4. When bucket is empty, requests are rejected

### Performance

Rate limiting overhead is minimal:
- O(1) per-request lookup using keyed rate limiter
- Concurrent access via lock-free data structures
- Periodic cleanup prevents memory growth

## Troubleshooting

### "Too Many Requests" Errors

**Cause**: Client is exceeding rate limits

**Solutions**:
1. Reduce request frequency
2. Implement client-side backoff
3. Contact node operator for increased limits
4. Use multiple IPs for load distribution (only if legitimate)

### Rate Limiting Not Working

**Check**:
1. Verify `max_requests_per_second > 0` in configuration
2. Check logs for startup warnings
3. Ensure `bind_address` is not localhost (if testing remotely)
4. Verify client IP is being correctly extracted

### Development Impact

**Problem**: Rate limiting interfering with development/testing

**Solution**: Disable in local configuration:
```toml
[rpc]
max_requests_per_second = 0
```

Or increase limits substantially:
```toml
[rpc]
max_requests_per_second = 10000
rate_limit_burst = 20000
```

## Migration from Older Versions

Rate limiting was refined in version 0.17.0 with the following changes:

- Switched from custom implementation to `governor` crate
- Added per-method tier-based limiting
- Fixed burst calculation (now explicit rather than doubled)
- Added comprehensive test coverage

No configuration migration is required. Default values provide sensible protection.

## References

- [JSON-RPC 2.0 Specification](https://www.jsonrpc.org/specification)
- [Governor Crate Documentation](https://docs.rs/governor)
- [Neo N3 RPC Documentation](https://docs.neo.org/docs/en-us/reference/rpc/latest-version/api.html)
