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
                let mut verifier = ent.verifier(inflater);
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
                let mut verifier = ent.verifier(inflater);
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
