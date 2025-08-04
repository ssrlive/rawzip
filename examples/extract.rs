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
    use rawzip::{path::ZipFilePath, CompressionMethod, ZipArchive, RECOMMENDED_BUFFER_SIZE};
    use std::io::{Error, ErrorKind::InvalidData};

    let file = std::fs::File::open(archive_path)?;
    let mut buffer = vec![0u8; RECOMMENDED_BUFFER_SIZE];
    let archive = ZipArchive::from_file(file, &mut buffer)
        .map_err(|e| Error::new(InvalidData, format!("Failed to read ZIP archive: {e}")))?;

    let mut written_paths = std::collections::HashSet::new();

    let mut entries = archive.entries(&mut buffer);
    while let Some(entry) = entries
        .next_entry()
        .map_err(|e| Error::new(InvalidData, format!("Failed to read entry: {e}")))?
    {
        let raw_path = entry.file_path();
        // Avoid directory traversal attacks by normalizing the path
        let file_path = match ZipFilePath::from_bytes(raw_path.as_ref()).try_normalize() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Skipped suspicious path: {raw_path:?}, reason: {e}");
                continue;
            }
        };
        let out_path = target_dir
            .as_ref()
            .join(std::path::PathBuf::from(&file_path.as_ref()));

        // Check for overlapping paths
        if !written_paths.insert(out_path.clone()) {
            eprintln!("Skipped overlapping path: {:?}", out_path);
            continue;
        }

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
                    continue;
                }
            }

            match entry.last_modified() {
                rawzip::time::ZipDateTimeKind::Utc(dt) => {
                    let mtime = filetime::FileTime::from_unix_time(dt.to_unix(), dt.nanosecond());
                    filetime::set_file_mtime(&out_path, mtime)?;
                }
                rawzip::time::ZipDateTimeKind::Local(_dt) => {
                    // need rawzip author's help
                    // let mtime = filetime::FileTime::from_unix_time(_dt.to_unix(), _dt.nanosecond());
                    // filetime::set_file_mtime(&out_path, mtime)?;
                }
            }

            let mode = entry.mode().value();
            let perms: std::fs::Permissions;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                perms = std::fs::Permissions::from_mode(mode);
            }
            #[cfg(windows)]
            {
                let readonly = (mode & 0o200) == 0;
                let mut _perms = std::fs::metadata(&out_path)?.permissions();
                _perms.set_readonly(readonly);
                perms = _perms;
            }
            std::fs::set_permissions(&out_path, perms)?;
        }
        // println!("Extracted: {out_path:?}");
    }

    Ok(())
}
