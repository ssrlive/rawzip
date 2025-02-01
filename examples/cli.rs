fn main() {
    let args = std::env::args().collect::<Vec<_>>();

    let Some(path) = args.get(1) else {
        eprintln!("Usage: {} <file>", args[0]);
        std::process::exit(1);
    };

    let fp = match std::fs::File::open(path) {
        Ok(fp) => fp,
        Err(e) => {
            eprintln!("Failed to open file {}: {}", path, e);
            std::process::exit(1);
        }
    };

    let mut buf = vec![0u8; rawzip::RECOMMENDED_BUFFER_SIZE];

    let archive = match rawzip::ZipArchive::from_file(fp, &mut buf[..]) {
        Ok(archive) => archive,
        Err(e) => {
            eprintln!("Failed to locate zip archive in file: {}", e);
            std::process::exit(1);
        }
    };

    let out = std::io::stdout();
    let mut out = out.lock();

    let mut entries = archive.entries(&mut buf);
    while let Ok(Some(entry)) = entries.next_entry() {
        if entry.is_dir() {
            continue;
        }

        let wayfinder = entry.wayfinder();
        let Ok(ent) = archive.get_entry(wayfinder) else {
            eprintln!("Failed to get entry");
            std::process::exit(1);
        };

        if entry.compression_method() != rawzip::CompressionMethod::Deflate {
            eprintln!(
                "Unsupported compression method: {:?}",
                entry.compression_method()
            );
            std::process::exit(1);
        }

        let reader = ent.reader();
        let inflater = flate2::read::DeflateDecoder::new(reader);
        let mut verifier = ent.verifying_reader(inflater);
        if let Err(e) = std::io::copy(&mut verifier, &mut out) {
            eprintln!("Failed to copy entry to data: {}", e);
            std::process::exit(1);
        }
    }
}
