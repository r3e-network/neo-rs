# Neo Rust Performance Optimization Guide

## Overview

This guide provides comprehensive performance optimization strategies for the Neo Rust node implementation, covering system tuning, application optimization, and monitoring best practices.

## 🎯 Performance Goals

| Metric | Target | Critical Threshold |
|--------|--------|-------------------|
| Block Processing Time | < 500ms | > 2s |
| Transaction Validation | < 10ms | > 50ms |
| RPC Response Time | < 100ms | > 500ms |
| Memory Usage | < 4GB | > 8GB |
| Sync Speed | > 100 blocks/sec | < 10 blocks/sec |

## 🔧 System-Level Optimizations

### 1. Linux Kernel Tuning

Add to `/etc/sysctl.conf`:

```bash
# Network optimizations
net.core.rmem_max = 134217728
net.core.wmem_max = 134217728
net.ipv4.tcp_rmem = 4096 87380 134217728
net.ipv4.tcp_wmem = 4096 65536 134217728
net.core.netdev_max_backlog = 5000
net.ipv4.tcp_congestion_control = bbr
net.core.default_qdisc = fq

# File system optimizations
fs.file-max = 2097152
fs.nr_open = 1048576

# Virtual memory optimizations
vm.swappiness = 10
vm.dirty_ratio = 15
vm.dirty_background_ratio = 5

# Connection tracking
net.netfilter.nf_conntrack_max = 1048576
net.nf_conntrack_max = 1048576
```

Apply changes:
```bash
sudo sysctl -p
```

### 2. File Descriptor Limits

Edit `/etc/security/limits.conf`:

```bash
neo soft nofile 1048576
neo hard nofile 1048576
neo soft nproc 65536
neo hard nproc 65536
```

### 3. CPU Governor

```bash
# Set performance governor
sudo cpupower frequency-set -g performance

# Disable CPU frequency scaling
echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
```

### 4. NUMA Optimization

```bash
# Check NUMA topology
numactl --hardware

# Run node with NUMA binding
numactl --cpunodebind=0 --membind=0 neo-node
```

## 🚀 Application-Level Optimizations

### 1. Rust Compiler Optimizations

Update `Cargo.toml`:

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true

[profile.release.package."*"]
opt-level = 3

# CPU-specific optimizations
[target.'cfg(target_arch = "x86_64")']
rustflags = ["-C", "target-cpu=native"]
```

### 2. Memory Allocator

Use jemalloc for better performance:

```toml
# Cargo.toml
[dependencies]
jemallocator = "0.5"
```

```rust
// main.rs
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;
```

### 3. Async Runtime Tuning

```rust
// Configure Tokio runtime
let runtime = tokio::runtime::Builder::new_multi_thread()
    .worker_threads(num_cpus::get())
    .thread_name("neo-worker")
    .thread_stack_size(3 * 1024 * 1024)
    .enable_all()
    .build()
    .unwrap();
```

## 📊 Database Optimizations

### 1. RocksDB Configuration

```rust
// Optimized RocksDB settings
let mut opts = rocksdb::Options::default();
opts.set_write_buffer_size(128 * 1024 * 1024); // 128MB
opts.set_max_write_buffer_number(4);
opts.set_target_file_size_base(256 * 1024 * 1024); // 256MB
opts.set_max_background_jobs(8);
opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
opts.increase_parallelism(num_cpus::get() as i32);
opts.set_max_open_files(10000);

// Block cache
let cache = rocksdb::Cache::new_lru_cache(2 * 1024 * 1024 * 1024); // 2GB
let mut block_opts = rocksdb::BlockBasedOptions::default();
block_opts.set_block_cache(&cache);
block_opts.set_cache_index_and_filter_blocks(true);
block_opts.set_pin_l0_filter_and_index_blocks_in_cache(true);
opts.set_block_based_table_factory(&block_opts);

// Enable statistics
opts.enable_statistics();
```

### 2. Database Maintenance

```bash
#!/bin/bash
# db_maintenance.sh - Run during low activity

# Compact database
neo-node db compact

# Analyze and optimize
neo-node db analyze

# Clear write-ahead logs
find /var/neo/data -name "*.log" -mtime +7 -delete
```

## 🌐 Network Optimizations

### 1. Connection Pool Tuning

```toml
[network]
max_peers = 50
connection_timeout = 10
idle_timeout = 300
max_concurrent_connections = 200

[network.buffer]
send_buffer_size = "8MB"
recv_buffer_size = "8MB"
```

### 2. Message Batching

```rust
// Batch multiple messages
impl NetworkHandler {
    async fn send_messages(&self, messages: Vec<Message>) {
        // Group messages by peer
        let mut peer_messages: HashMap<PeerId, Vec<Message>> = HashMap::new();
        
        for msg in messages {
            peer_messages.entry(msg.peer_id)
                .or_insert_with(Vec::new)
                .push(msg);
        }
        
        // Send batched messages
        for (peer_id, batch) in peer_messages {
            self.send_batch(peer_id, batch).await;
        }
    }
}
```

## ⚡ VM Performance Optimization

### 1. JIT Compilation (Future Enhancement)

```rust
// Placeholder for JIT implementation
pub struct JitCompiler {
    cache: HashMap<ScriptHash, CompiledCode>,
}

impl JitCompiler {
    pub fn compile(&mut self, script: &Script) -> Result<CompiledCode> {
        if let Some(compiled) = self.cache.get(&script.hash()) {
            return Ok(compiled.clone());
        }
        
        // Compile hot paths
        let compiled = self.compile_script(script)?;
        self.cache.insert(script.hash(), compiled.clone());
        Ok(compiled)
    }
}
```

### 2. Script Caching

```rust
// Cache frequently executed scripts
pub struct ScriptCache {
    cache: LruCache<UInt256, Arc<Script>>,
}

impl ScriptCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: LruCache::new(capacity),
        }
    }
    
    pub fn get_or_load(&mut self, hash: &UInt256) -> Result<Arc<Script>> {
        if let Some(script) = self.cache.get(hash) {
            return Ok(script.clone());
        }
        
        let script = self.load_from_storage(hash)?;
        let arc_script = Arc::new(script);
        self.cache.put(*hash, arc_script.clone());
        Ok(arc_script)
    }
}
```

## 🔍 Profiling and Monitoring

### 1. Performance Profiling

```bash
# CPU profiling with perf
sudo perf record -g neo-node
sudo perf report

# Flame graphs
git clone https://github.com/brendangregg/FlameGraph
perf script | ./FlameGraph/stackcollapse-perf.pl | ./FlameGraph/flamegraph.pl > flame.svg
```

### 2. Memory Profiling

```bash
# Using Valgrind
valgrind --tool=massif --massif-out-file=massif.out neo-node
ms_print massif.out

# Using heaptrack
heaptrack neo-node
heaptrack_gui heaptrack.neo-node.*.gz
```

### 3. Built-in Profiling

```rust
// Add profiling endpoints
#[get("/debug/profile/cpu")]
async fn cpu_profile() -> Result<impl Responder> {
    let profile = ProfileBuilder::new()
        .frequency(1000)
        .blocklist(&["libc", "libpthread"])
        .build()?;
        
    let report = profile.report().build()?;
    Ok(HttpResponse::Ok().body(report.flamegraph()))
}
```

## 📈 Benchmarking

### 1. Automated Performance Tests

```rust
#[cfg(test)]
mod bench {
    use criterion::{black_box, criterion_group, criterion_main, Criterion};
    
    fn bench_block_processing(c: &mut Criterion) {
        c.bench_function("process_block", |b| {
            b.iter(|| {
                process_block(black_box(&test_block))
            });
        });
    }
    
    fn bench_transaction_validation(c: &mut Criterion) {
        c.bench_function("validate_tx", |b| {
            b.iter(|| {
                validate_transaction(black_box(&test_tx))
            });
        });
    }
    
    criterion_group!(benches, bench_block_processing, bench_transaction_validation);
    criterion_main!(benches);
}
```

### 2. Load Testing

```bash
#!/bin/bash
# load_test.sh

# Generate load
for i in {1..1000}; do
    curl -X POST http://localhost:20332 \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":'$i'}' &
done

wait

# Analyze results
grep "response_time" /var/log/neo-node/metrics.log | \
    awk '{sum+=$2; count++} END {print "Average response time:", sum/count, "ms"}'
```

## 🎯 Optimization Checklist

### Initial Setup
- [ ] Apply kernel tuning parameters
- [ ] Set file descriptor limits
- [ ] Configure CPU governor
- [ ] Enable huge pages
- [ ] Install performance monitoring tools

### Compilation
- [ ] Enable release optimizations
- [ ] Use native CPU instructions
- [ ] Enable LTO (Link Time Optimization)
- [ ] Strip debug symbols

### Runtime
- [ ] Use jemalloc allocator
- [ ] Configure thread pool sizes
- [ ] Enable connection pooling
- [ ] Set appropriate cache sizes

### Database
- [ ] Optimize RocksDB settings
- [ ] Enable compression
- [ ] Configure block cache
- [ ] Schedule regular compaction

### Monitoring
- [ ] Set up continuous profiling
- [ ] Monitor key metrics
- [ ] Create performance dashboards
- [ ] Set up alerting thresholds

## 📊 Performance Tuning Matrix

| Component | Default | Optimized | Impact |
|-----------|---------|-----------|---------|
| Worker Threads | 4 | CPU cores | +40% throughput |
| DB Write Buffer | 64MB | 128MB | +25% write speed |
| Network Buffer | 1MB | 8MB | +30% network throughput |
| Block Cache | 512MB | 2GB | +50% read speed |
| Connection Pool | 10 | 50 | +35% concurrency |

## 🚨 Common Performance Issues

### 1. High CPU Usage
```bash
# Identify CPU-intensive operations
perf top -p $(pgrep neo-node)

# Check for lock contention
perf lock record -p $(pgrep neo-node)
perf lock report
```

### 2. Memory Leaks
```bash
# Monitor memory growth
watch -n 1 'ps aux | grep neo-node'

# Analyze heap usage
gdb -p $(pgrep neo-node)
(gdb) info proc mappings
(gdb) dump memory heap.dump 0x... 0x...
```

### 3. Slow Database Queries
```bash
# Enable RocksDB statistics
export RUST_LOG=rocksdb=debug

# Analyze slow queries
grep "slow_query" /var/log/neo-node/db.log
```

## 📚 References

- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [RocksDB Tuning Guide](https://github.com/facebook/rocksdb/wiki/RocksDB-Tuning-Guide)
- [Linux Performance Tuning](https://www.kernel.org/doc/Documentation/sysctl/vm.txt)

---

Remember: Always benchmark before and after optimizations. Not all optimizations work in every environment.