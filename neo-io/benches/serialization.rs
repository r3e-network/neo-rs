use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::{UInt256, UINT256_SIZE};

// Benchmark payload: typical transaction-like structure
#[derive(Clone)]
struct BenchPayload {
    hash: UInt256,
    index: u32,
    data: Vec<u8>,
}

impl Serializable for BenchPayload {
    fn size(&self) -> usize {
        UINT256_SIZE + 4 + neo_io::serializable::helper::get_var_size_bytes(&self.data)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> neo_io::IoResult<()> {
        writer.write_serializable(&self.hash)?;
        writer.write_u32(self.index)?;
        writer.write_var_bytes(&self.data)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> neo_io::IoResult<Self> {
        Ok(Self {
            hash: UInt256::deserialize(reader)?,
            index: reader.read_u32()?,
            data: reader.read_var_bytes(1024)?,
        })
    }
}

fn create_payload(data_size: usize) -> BenchPayload {
    BenchPayload {
        hash: UInt256::zero(),
        index: 42,
        data: vec![0xAB; data_size],
    }
}

fn bench_single_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_serialization");

    for size in [32, 256, 1024] {
        let payload = create_payload(size);
        group.throughput(Throughput::Bytes((payload.size()) as u64));

        group.bench_with_input(
            BenchmarkId::new("serialize", size),
            &payload,
            |b, payload| {
                b.iter(|| {
                    let mut writer = BinaryWriter::new();
                    payload.serialize(&mut writer).unwrap();
                    black_box(writer.into_bytes())
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("serialize_with_capacity", size),
            &payload,
            |b, payload| {
                b.iter(|| {
                    let mut writer = BinaryWriter::with_capacity(payload.size());
                    payload.serialize(&mut writer).unwrap();
                    black_box(writer.into_bytes())
                });
            },
        );
    }

    group.finish();
}

fn bench_array_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("array_serialization");

    for count in [10, 100, 1000] {
        let payloads: Vec<BenchPayload> = (0..count).map(|_| create_payload(64)).collect();
        let total_size: usize = payloads.iter().map(|p| p.size()).sum::<usize>()
            + neo_io::var_int::encoded_len(count as u64);

        group.throughput(Throughput::Bytes(total_size as u64));

        group.bench_with_input(
            BenchmarkId::new("serialize_array", count),
            &payloads,
            |b, payloads| {
                b.iter(|| {
                    let mut writer = BinaryWriter::new();
                    writer.write_serializable_vec(payloads).unwrap();
                    black_box(writer.into_bytes())
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("serialize_array_with_size", count),
            &payloads,
            |b, payloads| {
                b.iter(|| {
                    let mut writer = BinaryWriter::new();
                    writer.write_serializable_vec_with_size(payloads).unwrap();
                    black_box(writer.into_bytes())
                });
            },
        );
    }

    group.finish();
}

fn bench_var_int_encoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("var_int");

    for value in [0u64, 252, 0xFFFF, 0xFFFF_FFFF, u64::MAX] {
        group.bench_with_input(
            BenchmarkId::new("encode", value),
            &value,
            |b, &value| {
                b.iter(|| {
                    let mut buf = Vec::new();
                    neo_io::var_int::write_var_int(value, &mut buf);
                    black_box(buf)
                });
            },
        );
    }

    group.finish();
}

fn bench_freeze(c: &mut Criterion) {
    let payload = create_payload(1024);

    c.bench_function("freeze_to_bytes", |b| {
        b.iter(|| {
            let mut writer = BinaryWriter::new();
            payload.serialize(&mut writer).unwrap();
            black_box(writer.freeze())
        });
    });
}

criterion_group!(
    benches,
    bench_single_serialization,
    bench_array_serialization,
    bench_var_int_encoding,
    bench_freeze
);
criterion_main!(benches);
