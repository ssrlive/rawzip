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

        if entry.compression_method() != rawzip::CompressionMethod::Deflate {
            continue;
        }

        let inflater = flate2::read::DeflateDecoder::new(ent.reader());
        let mut verifier = rawzip::ZipVerifier::new(inflater);
        let mut sink = std::io::sink();
        let Ok(_) = std::io::copy(&mut verifier, &mut sink) else {
            continue;
        };
    
        let claim = verifier.verification_claim();
        let reader = verifier.into_inner().into_inner();
    
        let _ = reader.verify_claim(claim);
    }
});
