#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut buf = vec![0u8; rawzip::RECOMMENDED_BUFFER_SIZE];
    let Ok(archive) = rawzip::ZipArchive::from_seekable(std::io::Cursor::new(data), &mut buf) else {
        return;
    };

    let mut entries = archive.entries(&mut buf);
    while let Ok(Some(entry)) = entries.next_entry() {
        if entry.is_dir() {
            continue;
        };

        let _name = entry.file_safe_path();
        let position = entry.wayfinder();
        let Ok(ent) = archive.get_entry(position) else {
            continue;
        };

        match entry.compression_method() {
            rawzip::CompressionMethod::Store => {
                let mut verifier = ent.verifying_reader(ent.reader());
                let mut sink = std::io::sink();
                let Ok(_) = std::io::copy(&mut verifier, &mut sink) else {
                    continue;
                };
            }
            rawzip::CompressionMethod::Deflate => {
                let inflater = flate2::read::DeflateDecoder::new(ent.reader());
                let mut verifier = ent.verifying_reader(inflater);
                let mut sink = std::io::sink();
                let Ok(_) = std::io::copy(&mut verifier, &mut sink) else {
                    continue;
                };
            }
            _ => continue,
        }
    }

    let archive = rawzip::ZipArchive::from_slice(data).unwrap();
    let mut entries = archive.entries();
    while let Ok(Some(entry)) = entries.next_entry() {
        if entry.is_dir() {
            continue;
        };

        let _name = entry.file_safe_path();
        let position = entry.wayfinder();
        let Ok(ent) = archive.get_entry(position) else {
            continue;
        };

        match entry.compression_method() {
            rawzip::CompressionMethod::Store => {
                let mut verifier = ent.verifying_reader(ent.data());
                let mut sink = std::io::sink();
                let Ok(_) = std::io::copy(&mut verifier, &mut sink) else {
                    continue;
                };
            }
            rawzip::CompressionMethod::Deflate => {
                let inflater = flate2::read::DeflateDecoder::new(ent.data());
                let mut verifier = ent.verifying_reader(inflater);
                let mut sink = std::io::sink();
                let Ok(_) = std::io::copy(&mut verifier, &mut sink) else {
                    continue;
                };
            }
            _ => continue,
        }
    }
});
