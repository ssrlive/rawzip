use std::sync::mpsc::{channel, sync_channel};
use std::sync::{Arc, Mutex};
use rayon::iter::ParallelBridge;
use rayon::prelude::*;
use std::io::Read;

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let Some(method) = args.get(1) else {
        eprintln!("Usage: {} <method> <file>", args[0]);
        std::process::exit(1);
    };

    let Some(path) = args.get(2) else {
        eprintln!("Usage: {} <file>", args[0]);
        std::process::exit(1);
    };

    match method.as_str() {
        "raw" => {
            let now = std::time::Instant::now();
            let result = rawzip_impl(path).unwrap();
            println!("rawzip: {:?} {}", now.elapsed(), result);
        }
        "rc" => {
            let now = std::time::Instant::now();
            let result = rc_zip(path).unwrap();
            println!("rc_zip: {:?} {}", now.elapsed(), result);
        }
        "zip" => {
            let now = std::time::Instant::now();
            let result = zip(path).unwrap();
            println!("zip: {:?} {}", now.elapsed(), result);
        }
        "async" => {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let now = std::time::Instant::now();
            let result = rt.block_on(async_zip(path)).unwrap();
            println!("async_zip: {:?} {}", now.elapsed(), result);
        }
        _ => {
            eprintln!("Invalid method: {}", method);
            std::process::exit(1);
        }
    }
}

fn rawzip_impl<P: AsRef<std::path::Path>>(fp: P) -> anyhow::Result<usize> {
    let mut buffer = vec![0u8; rawzip::RECOMMENDED_BUFFER_SIZE];
    let file = std::fs::File::open(fp)?;
    let mut archive = rawzip::ZipLocator::new().locate_in_reader(file, &mut buffer)?;
    let entries_hint = archive.entries_hint();
    let mut entries = archive.entries(&mut buffer);
    let mut positions = Vec::with_capacity(entries_hint as usize);

    let mut counter = 0;
    while let Some(entry) = entries.next_entry()? {
        if entry.file_name.last() == Some(&b'/') {
            continue;
        }

        positions.push(entry.positioning());
    }

    let parallelism = std::thread::available_parallelism()
        .map(|x| x.get().max(2))
        .unwrap_or(2);

    let (tx, rx) = sync_channel::<(Vec<u8>, usize)>(parallelism - 1);
    let (return_buf, receive_buf) = channel::<Vec<u8>>();

    std::thread::scope(|scope| {
        scope.spawn(move || {
            for position in positions {
                let Ok(x) = archive.get_entry(position, &mut buffer) else {
                    panic!("EEEK");
                };

                let size = position.compressed_size() as usize;
                let mut buf = if let Ok(mut existing_buf) = receive_buf.try_recv() {
                    existing_buf.resize(size, 0);
                    existing_buf
                } else {
                    vec![0u8; size]
                };

                x.reader().read_exact(&mut buf).unwrap();
                tx.send((buf, position.uncompressed_size() as usize)).unwrap();
            }
        });

        let data = rx.into_iter().par_bridge().map_with(Vec::<u8>::new(), |mut data, args| {
            let (deflated, uncompressed_size) = args;
            data.resize(uncompressed_size, 0);

            let inflation = libdeflater::Decompressor::new().deflate_decompress(&deflated, &mut data).unwrap();
            if rawzip::crc32(data.as_slice()) == inflation as u32 {
                panic!("HAHA");
            }
            let _ = return_buf.send(deflated);
        });

        println!("{}", data.count());
    });

    Ok(counter)
}

fn rc_zip<P: AsRef<std::path::Path>>(fp: P) -> anyhow::Result<usize> {
    use rc_zip_sync::ReadZip;

    let file = std::fs::File::open(fp)?;
    let reader = file.read_zip()?;

    let mut counter = 0;
    for _ in reader.entries() {
        counter += 1;
    }

    Ok(counter)
}

fn zip<P: AsRef<std::path::Path>>(fp: P) -> anyhow::Result<usize> {
    use zip::ZipArchive;

    let file = std::fs::File::open(fp)?;
    let archive = ZipArchive::new(file)?;
    let entries = archive.len();

    let archive = Arc::new(Mutex::new(archive));

    let dd = (0..entries)
        .par_bridge()
        .map_with((Vec::new(), archive.clone()), |(data, archive), i| {
            let mut lock = archive.lock().unwrap();
            let mut entry = lock.by_index(i).unwrap();
            data.resize(entry.size() as usize, 0);
            entry.read_exact(data).unwrap();
        })
        .count();
    // let mut buf = Vec::new();
    // for i in 0..archive.len() {
    //     let mut abc = archive.by_index(i).unwrap();
    //     buf.resize(abc.size() as usize, 0);
    //     abc.read_exact(&mut buf).unwrap();
    // }

    // std::thread::scope(|s| {
    //     s.spawn(|| {
    //         let abc = archive.by_index(0).unwrap();
    //     });
    // });
    Ok(dd)
}

async fn async_zip<P: AsRef<std::path::Path>>(fp: P) -> anyhow::Result<usize> {
    use async_zip::base::read::seek::ZipFileReader;
    use tokio_util::compat::TokioAsyncReadCompatExt;
    use tokio::{fs::File, io::BufReader};

    let data = BufReader::new( File::open(fp).await?);
    let reader = ZipFileReader::new(data.compat()).await?;
    Ok(reader.file().entries().len())
}