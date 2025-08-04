fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = <Args as ::clap::Parser>::parse();
    let default = args.verbosity.to_string();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default)).init();

    let archive = &args.archive;
    log::info!("Starting extraction of archive {archive:?}...");

    extract_zip_archive(archive, &args.target_dir)?;

    log::info!("Archive {archive:?} extraction completed successfully");
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
                let name = String::from_utf8_lossy(raw_path.as_ref()).to_string();
                log::warn!("Skipped suspicious path: {name:?}, reason: {e}",);
                continue;
            }
        };
        let relative_path = &std::path::PathBuf::from(&file_path.as_ref());
        let out_path = target_dir.as_ref().join(relative_path);

        // Check for overlapping paths
        if !written_paths.insert(out_path.clone()) {
            log::warn!("Skipped overlapping path: {out_path:?}");
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
                    log::error!("Unsupported compression method {method:?} for {relative_path:?}");
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
            #[allow(unused_mut)]
            let mut perms: std::fs::Permissions;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                perms = std::fs::Permissions::from_mode(mode);
            }
            #[cfg(windows)]
            {
                let readonly = (mode & 0o200) == 0;
                perms = std::fs::metadata(&out_path)?.permissions();
                perms.set_readonly(readonly);
            }
            std::fs::set_permissions(&out_path, perms)?;
        }
        log::trace!("Extracted: {relative_path:?}");
    }

    Ok(())
}

/// Extract example for the Rust rawzip crate
#[derive(Debug, Clone, clap::Parser)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Path to the ZIP archive to extract
    #[arg(short, long, value_name = "PATH")]
    pub archive: std::path::PathBuf,

    /// Directory to extract files into
    #[arg(short, long, value_name = "DIR")]
    pub target_dir: std::path::PathBuf,

    /// Verbosity level for logging
    #[arg(short, long, value_name = "level", value_enum, default_value = "info")]
    pub verbosity: ArgVerbosity,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum)]
pub enum ArgVerbosity {
    Off = 0,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl std::fmt::Display for ArgVerbosity {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ArgVerbosity::Off => write!(f, "off"),
            ArgVerbosity::Error => write!(f, "error"),
            ArgVerbosity::Warn => write!(f, "warn"),
            ArgVerbosity::Info => write!(f, "info"),
            ArgVerbosity::Debug => write!(f, "debug"),
            ArgVerbosity::Trace => write!(f, "trace"),
        }
    }
}
