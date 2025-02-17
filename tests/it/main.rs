use quickcheck_macros::quickcheck;
use std::io::Cursor;

#[derive(Debug, PartialEq)]
struct TestZip {
    file_name: String,
    data: Compression,
}

#[derive(Debug, PartialEq)]
enum Compression {
    Deflated(Vec<u8>),
}

#[test]
fn zip_integration_tests() {
    let f = std::fs::File::open("assets/zip64.zip").unwrap();
    let mut buf = vec![0u8; rawzip::RECOMMENDED_BUFFER_SIZE];
    let archive = rawzip::ZipArchive::from_file(f, &mut buf[..]).unwrap();
    let mut entries = archive.entries(&mut buf);
    let mut actual = vec![];
    while let Some(entry) = entries.next_entry().unwrap() {
        if entry.is_dir() {
            continue;
        }

        let file_name = entry.file_safe_path().unwrap();

        let position = entry.wayfinder();
        let ent = archive.get_entry(position).unwrap();

        match entry.compression_method() {
            rawzip::CompressionMethod::Deflate => {
                let inflater = flate2::read::DeflateDecoder::new(ent.reader());
                let mut verifier = ent.verifying_reader(inflater);
                let mut data = Vec::new();
                std::io::copy(&mut verifier, &mut Cursor::new(&mut data)).unwrap();
                actual.push(TestZip {
                    file_name: file_name.into_owned(),
                    data: Compression::Deflated(data),
                });
            }
            _ => todo!(),
        }
    }

    assert_eq!(
        actual,
        vec![TestZip {
            file_name: String::from("README"),
            data: Compression::Deflated(b"This small file is in ZIP64 format.\n".to_vec()),
        },]
    );
}

#[test]
fn zip_integration_tests_slice() {
    let data = std::fs::read("assets/zip64.zip").unwrap();
    let archive = rawzip::ZipArchive::from_slice(&data).unwrap();
    let mut entries = archive.entries();
    let mut actual = vec![];
    while let Some(entry) = entries.next_entry().unwrap() {
        if entry.is_dir() {
            continue;
        }

        let file_name = entry.file_safe_path().unwrap();

        let position = entry.wayfinder();
        let ent = archive.get_entry(position).unwrap();

        match entry.compression_method() {
            rawzip::CompressionMethod::Deflate => {
                let inflater = flate2::read::DeflateDecoder::new(ent.data());
                let mut verifier = ent.verifying_reader(inflater);
                let mut data = Vec::new();
                std::io::copy(&mut verifier, &mut Cursor::new(&mut data)).unwrap();
                actual.push(TestZip {
                    file_name: file_name.into_owned(),
                    data: Compression::Deflated(data),
                });
            }
            _ => todo!(),
        }
    }

    assert_eq!(
        actual,
        vec![TestZip {
            file_name: String::from("README"),
            data: Compression::Deflated(b"This small file is in ZIP64 format.\n".to_vec()),
        },]
    );
}

#[quickcheck]
fn test_read_what_we_write_slice(data: Vec<u8>) {
    let mut output = Vec::new();
    {
        let mut archive = rawzip::ZipArchiveWriter::new(&mut output);
        let mut file = archive
            .new_file("file.txt", rawzip::ZipEntryOptions::default())
            .unwrap();
        let mut writer = rawzip::RawZipWriter::new(&mut file);
        std::io::copy(&mut Cursor::new(&data), &mut writer).unwrap();
        let (_, descriptor) = writer.finish().unwrap();
        assert_eq!(descriptor.uncompressed_size(), data.len() as u64);
        let compressed = file.finish(descriptor).unwrap();
        assert_eq!(compressed, data.len() as u64);
        archive.finish().unwrap();
    }

    let archive = rawzip::ZipArchive::from_slice(&output).unwrap();
    let mut entries = archive.entries();
    let entry = entries.next_entry().unwrap().unwrap();
    assert_eq!(entry.file_safe_path().unwrap(), "file.txt");
    assert_eq!(entry.compression_method(), rawzip::CompressionMethod::Store);
    assert_eq!(entry.uncompressed_size_hint(), data.len() as u64);
    assert_eq!(entry.compressed_size_hint(), data.len() as u64);
    let wayfinder = entry.wayfinder();
    let entry = archive.get_entry(wayfinder).unwrap();
    let mut actual = Vec::new();
    std::io::copy(&mut entry.data(), &mut Cursor::new(&mut actual)).unwrap();
    assert_eq!(data, actual);
}
