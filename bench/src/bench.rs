use criterion::{BenchmarkId, Criterion, Throughput};
use std::io::{Cursor, Write};

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

fn eocd(c: &mut Criterion) {
    let mut group = c.benchmark_group("eocd-locator");
    for size in &[1, 4, 16, 64, 256, 1024, 4096, 16384, 65536] {
        let data = vec![4; *size];
        let input = data.as_slice();
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _size| {
            b.iter(|| rawzip::ZipArchive::from_slice(&input));
        });
    }
    group.finish();
}

fn create_test_zip() -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    let mut archive = rawzip::ZipArchiveWriter::new(&mut output);
    
    for i in 0..200_000 {
        let filename = format!("file{:06}.txt", i);
        let options = rawzip::ZipEntryOptions::default()
            .compression_method(rawzip::CompressionMethod::Store);
        
        let mut file = archive.new_file(&filename, options).unwrap();
        let mut writer = rawzip::ZipDataWriter::new(&mut file);
        writer.write_all(b"x").unwrap();
        let (_, descriptor) = writer.finish().unwrap();
        file.finish(descriptor).unwrap();
    }
    
    archive.finish().unwrap();
    output.into_inner()
}

fn entries(c: &mut Criterion) {
    let zip_data = create_test_zip();
    
    c.bench_function("entries", |b| {
        b.iter(|| {
            let archive = rawzip::ZipArchive::from_slice(&zip_data).unwrap();
            let mut total_size = 0u64;
            for entry_result in archive.entries() {
                let entry = entry_result.unwrap();
                total_size += entry.uncompressed_size_hint();
            }
            assert_eq!(total_size, 200_000);
        })
    });
}

criterion::criterion_group!(benches, crc32, eocd, entries);
criterion::criterion_main!(benches);
