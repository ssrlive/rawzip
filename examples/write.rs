use rawzip::{ZipArchiveWriter, ZipDataWriter, ZipEntryOptions};
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::UNIX_EPOCH;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: {} <output.zip> <input_path>...", args[0]);
        eprintln!("Create a ZIP archive from the specified files and directories");
        std::process::exit(1);
    }

    let output_path = &args[1];
    let input_paths: Vec<&str> = args[2..].iter().map(|s| s.as_str()).collect();

    let output_file = File::create(output_path)?;
    let writer = std::io::BufWriter::new(output_file);
    let mut archive = ZipArchiveWriter::new(writer);

    for input_path in input_paths {
        let path = Path::new(input_path);
        if path.is_file() {
            add_file_to_archive(
                &mut archive,
                path,
                path.file_name().unwrap().to_str().unwrap(),
            )?;
        } else if path.is_dir() {
            add_directory_to_archive(&mut archive, path, "")?;
        } else {
            eprintln!(
                "Warning: '{}' does not exist or is not a regular file/directory",
                input_path
            );
        }
    }

    archive.finish()?;
    println!("Successfully created '{}'", output_path);
    Ok(())
}

fn create_entry_options(file_path: &Path) -> Result<ZipEntryOptions, Box<dyn std::error::Error>> {
    let metadata = fs::metadata(file_path)?;
    let modified = metadata.modified()?;

    // Convert system time to UTC DateTime
    let unix_seconds = modified.duration_since(UNIX_EPOCH)?.as_secs() as i64;
    let utc_datetime = rawzip::time::UtcDateTime::from_unix(unix_seconds);

    let options = ZipEntryOptions::default().modification_time(utc_datetime);
    let options = match get_unix_permissions(&metadata) {
        Some(permissions) => options.unix_permissions(permissions),
        None => options,
    };

    Ok(options)
}

fn add_file_to_archive<W: Write>(
    archive: &mut ZipArchiveWriter<W>,
    file_path: &Path,
    archive_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let options =
        create_entry_options(file_path)?.compression_method(rawzip::CompressionMethod::Deflate);

    let mut file = archive.new_file(archive_path, options)?;

    // Read and compress the file content using Deflate
    let file_content = fs::read(file_path)?;
    let encoder = flate2::write::DeflateEncoder::new(&mut file, flate2::Compression::default());
    let mut writer = ZipDataWriter::new(encoder);
    writer.write_all(&file_content)?;
    let (encoder, output) = writer.finish()?;
    encoder.finish()?;
    file.finish(output)?;

    println!("  adding: {}", archive_path);
    Ok(())
}

fn add_directory_to_archive<W: Write>(
    archive: &mut ZipArchiveWriter<W>,
    dir_path: &Path,
    base_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let entries = fs::read_dir(dir_path)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_str().unwrap();

        let archive_path = if base_path.is_empty() {
            name_str.to_string()
        } else {
            format!("{}/{}", base_path, name_str)
        };

        if path.is_file() {
            add_file_to_archive(archive, &path, &archive_path)?;
        } else if path.is_dir() {
            // Add directory entry
            let options = create_entry_options(&path)?;

            let dir_archive_path = format!("{}/", archive_path);
            archive.new_dir(&dir_archive_path, options)?;
            println!("  adding: {}", dir_archive_path);

            // Recursively add directory contents
            add_directory_to_archive(archive, &path, &archive_path)?;
        }
    }

    Ok(())
}

#[cfg(unix)]
fn get_unix_permissions(metadata: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(metadata.permissions().mode())
}

#[cfg(not(unix))]
fn get_unix_permissions(metadata: &fs::Metadata) -> Option<u32> {
    None
}
