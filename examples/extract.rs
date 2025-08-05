//! This example demonstrates how to safely extract ZIP archives. It implements
//! several security measures to prevent common ZIP-based attacks while
//! providing a basic ZIP extraction. Limitations of this example (but not of
//! rawzip).
//!
//! - Supports only store and deflate compression methods
//! - Supports only UTF-8 file paths

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 || args.len() > 4 {
        eprintln!(
            "Usage: {} <archive.zip> <target_dir> [--force-suspicious]",
            args[0]
        );
        std::process::exit(1);
    }

    let archive_path = &args[1];
    let target_dir = &args[2];
    let force_extract_suspicious = args.get(3).map_or(false, |s| s == "--force-suspicious");
    extract_zip_archive(archive_path, target_dir, force_extract_suspicious)?;
    Ok(())
}

fn extract_zip_archive<P: AsRef<std::path::Path>>(
    archive_path: P,
    target_dir: P,
    force_extract_suspicious: bool,
) -> std::io::Result<()> {
    use rawzip::{CompressionMethod, ZipArchive, RECOMMENDED_BUFFER_SIZE};
    use std::io::{Error, ErrorKind::InvalidData};

    let archive_path = archive_path.as_ref();
    let target_dir = target_dir.as_ref();

    let file = std::fs::File::open(archive_path)?;
    let mut buffer = vec![0u8; RECOMMENDED_BUFFER_SIZE];
    let archive = ZipArchive::from_file(file, &mut buffer).map_err(|e| {
        let path = archive_path.display();
        let error = format!("Failed to read ZIP archive: {path:?}, error: {e}");
        Error::new(InvalidData, error)
    })?;

    // Maintain sorted list of compressed data ranges to detect overlaps:
    // https://www.bamsoftware.com/hacks/zipbomb/
    let mut compressed_ranges = Vec::new();

    let mut entries = archive.entries(&mut buffer);
    while let Some(entry) = entries
        .next_entry()
        .map_err(|e| Error::new(InvalidData, format!("Failed to read ZIP entry: {e}")))?
    {
        let raw_path = entry.file_path();

        // Avoid zip slips by normalizing the path. Note that it is not required for
        // zip file paths to be UTF-8
        let relative_path = match raw_path.try_normalize() {
            Ok(p) => std::path::PathBuf::from(p.as_ref()),
            Err(e) => {
                let raw_path_str = String::from_utf8_lossy(raw_path.as_ref());
                if force_extract_suspicious {
                    eprintln!("Force extracting suspicious path: {raw_path_str:?}, reason: {e}");
                    std::path::PathBuf::from(raw_path_str.as_ref())
                } else {
                    eprintln!("Skipped suspicious path: {raw_path_str:?}, reason: {e}");
                    continue;
                }
            }
        };

        let out_path = target_dir.join(&relative_path);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let zip_entry = archive.get_entry(entry.wayfinder()).map_err(|e| {
            let error = format!("Failed to get ZIP entry for file: {relative_path:?}, error: {e}");
            Error::new(InvalidData, error)
        })?;
        let reader = zip_entry.reader();

        // Check for overlapping compressed data ranges
        let current_range = zip_entry.compressed_data_range();
        let (current_start, current_end) = current_range;

        // Find insertion point to maintain sorted order by start offset
        let insert_pos = compressed_ranges
            .binary_search_by_key(&current_start, |&(start, _)| start)
            .unwrap_or_else(|pos| pos);

        // Check overlap with previous range (if exists)
        if insert_pos > 0 {
            let (_, prev_end) = compressed_ranges[insert_pos - 1];
            if prev_end > current_start {
                eprintln!("Skipped file with overlapping compressed data: {relative_path:?} (range {current_start}..{current_end} overlaps with previous range ending at {prev_end})");
                continue;
            }
        }

        // Check overlap with next range (if exists)
        if insert_pos < compressed_ranges.len() {
            let (next_start, _) = compressed_ranges[insert_pos];
            if current_end > next_start {
                eprintln!("Skipped file with overlapping compressed data: {relative_path:?} (range {current_start}..{current_end} overlaps with next range starting at {next_start})");
                continue;
            }
        }

        // Insert the range at the correct position to maintain sorted order
        compressed_ranges.insert(insert_pos, current_range);

        // "DEFLATE, the compression algorithm most commonly supported by zip
        // parsers, cannot achieve a compression ratio greater than 1032"
        // https://www.bamsoftware.com/hacks/zipbomb/
        let compressed_size = entry.compressed_size_hint();
        let uncompressed_size = entry.uncompressed_size_hint();
        if compressed_size > 0 && uncompressed_size / compressed_size > 1032 {
            eprintln!("Skipped potential zip bomb: compression ratio {:.1}:1 exceeds limit of 1032:1 for file: {relative_path:?}", 
                         uncompressed_size as f64 / compressed_size as f64);
            continue;
        }

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
                eprintln!("Unsupported compression method {method:?} for file: {relative_path:?}");
                continue;
            }
        }

        match entry.last_modified() {
            rawzip::time::ZipDateTimeKind::Utc(dt) => {
                let mtime = filetime::FileTime::from_unix_time(dt.to_unix(), dt.nanosecond());
                filetime::set_file_mtime(&out_path, mtime)?;
            }
            rawzip::time::ZipDateTimeKind::Local(dt) if dt.year() > 1980 => {
                // We only want to write out timestamps that are more recent
                // than 1980 (which is the start date for the msdos timestamp
                // format used in zip files).

                // Convert local time to UTC by treating it as it was UTC. This
                // is something you may (or may not) want to do too.
                let utc_time = rawzip::time::UtcDateTime::from_components(
                    dt.year(),
                    dt.month(),
                    dt.day(),
                    dt.hour(),
                    dt.minute(),
                    dt.second(),
                    dt.nanosecond(),
                );

                match utc_time {
                    Some(utc_time) => {
                        let mtime = filetime::FileTime::from_unix_time(
                            utc_time.to_unix(),
                            utc_time.nanosecond(),
                        );
                        filetime::set_file_mtime(&out_path, mtime)?;
                    }
                    None => {
                        eprintln!("Invalid local time for file: {relative_path:?}, skipping timestamp setting");
                    }
                }
            }

            _ => {}
        };

        // Set file attributes based on platform
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = entry.mode();
            let perms = std::fs::Permissions::from_mode(mode.permissions());
            std::fs::set_permissions(&out_path, perms)?;
        }

        #[cfg(windows)]
        {
            // Detect if the file should be marked as readonly
            if entry.mode().permissions() & 0o200 == 0 {
                let mut perms = std::fs::metadata(&out_path)?.permissions();
                perms.set_readonly(true);
                std::fs::set_permissions(&out_path, perms)?;
            }
        }
    }

    Ok(())
}
