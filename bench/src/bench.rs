use criterion::{BenchmarkId, Criterion, Throughput};

fn crc32(c: &mut Criterion) {
    let mut group = c.benchmark_group("crc32");
    for size in &[1, 4, 16, 64, 256, 1024, 4096, 16384, 65536] {
        let data = vec![0; *size];
        let input = data.as_slice();
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _size| {
            b.iter(|| rawzip::crc32(input));
        });
    }
    group.finish();
}

criterion::criterion_group!(benches, crc32);
criterion::criterion_main!(benches);
