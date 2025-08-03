fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <archive.zip> <target_dir>", args[0]);
        std::process::exit(1);
    }

    let archive_path = &args[1];
    let target_dir = &args[2];
    extract_zip_archive(archive_path, target_dir)?;
    Ok(())
}

fn extract_zip_archive<P: AsRef<std::path::Path>>(
    archive_path: P,
    target_dir: P,
) -> std::io::Result<()> {
    use rawzip::{CompressionMethod, ZipArchive, RECOMMENDED_BUFFER_SIZE};
    use std::io::{Error, ErrorKind::InvalidData};

    let file = std::fs::File::open(archive_path)?;
    let mut buffer = vec![0u8; RECOMMENDED_BUFFER_SIZE];
    let archive = ZipArchive::from_file(file, &mut buffer)
        .map_err(|e| Error::new(InvalidData, format!("Failed to read ZIP archive: {e}")))?;

    let mut entries = archive.entries(&mut buffer);
    while let Some(entry) = entries
        .next_entry()
        .map_err(|e| Error::new(InvalidData, format!("Failed to read entry: {e}")))?
    {
        let file_path = String::from_utf8_lossy(entry.file_path().as_ref()).to_string();
        let out_path = target_dir
            .as_ref()
            .join(std::path::PathBuf::from(&file_path));

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let zip_entry = archive
                .get_entry(entry.wayfinder())
                .map_err(|e| Error::new(InvalidData, format!("Failed to get entry: {e}")))?;
            let reader = zip_entry.reader();

            let mut outfile = std::fs::File::create(&out_path)?;
            let method = entry.compression_method();
            match method {
                CompressionMethod::Store => {
                    let mut verifier = zip_entry.verifying_reader(reader);
                    std::io::copy(&mut verifier, &mut outfile)?;
                }
                CompressionMethod::Deflate => {
                    let inflater = flate2::read::DeflateDecoder::new(reader);
                    let mut verifier = zip_entry.verifying_reader(inflater);
                    std::io::copy(&mut verifier, &mut outfile)?;
                }
                _ => {
                    eprintln!("Unsupported compression method {method:?} for file: {file_path:?}");
                }
            }
        }
        // println!("Extracted: {out_path:?}");
    }

    Ok(())
}
