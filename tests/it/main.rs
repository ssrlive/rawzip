use quickcheck_macros::quickcheck;
use rawzip::{Error, ErrorKind};
use std::fs::File;
use std::io::Cursor;
use std::path::Path;

macro_rules! zip_test_case {
    ($name:expr, $case:expr) => {
        paste::paste! {
            #[test]
            fn [<test_ $name _reader >]() {
                run_zip_test_case_reader(&$case);
            }

            #[test]
            fn [<test_ $name _slice >]() {
                run_zip_test_case_slice(&$case);
            }
        }
    };
}

#[derive(Debug, Default)]
struct ZipTestCase {
    name: &'static str,
    comment: Option<&'static [u8]>,
    files: Vec<ZipTestFileEntry>,
    expected_error_kind: Option<ErrorKind>,
}

#[derive(Debug)]
struct ZipTestFileEntry {
    name: &'static str,
    expected_content: ExpectedContent,
}

#[derive(Debug)]
enum ExpectedContent {
    Content(Vec<u8>),
    File(&'static str),
    // Size(u64),
}

zip_test_case!(
    "test",
    ZipTestCase {
        name: "test.zip",
        comment: Some(b"This is a zipfile comment."),
        files: vec![
            ZipTestFileEntry {
                name: "test.txt",
                expected_content: ExpectedContent::Content(b"This is a test text file.\n".to_vec(),),
            },
            ZipTestFileEntry {
                name: "gophercolor16x16.png",
                expected_content: ExpectedContent::File("gophercolor16x16.png"),
            },
        ],
        ..Default::default()
    }
);

zip_test_case!(
    "readme_notzip",
    ZipTestCase {
        name: "readme.notzip",
        expected_error_kind: Some(ErrorKind::MissingEndOfCentralDirectory),
        ..Default::default()
    }
);

zip_test_case!(
    "test_trailing_junk",
    ZipTestCase {
        name: "test-trailing-junk.zip",
        comment: Some(b"This is a zipfile comment."),
        files: vec![
            ZipTestFileEntry {
                name: "test.txt",
                expected_content: ExpectedContent::Content(b"This is a test text file.\n".to_vec(),),
            },
            ZipTestFileEntry {
                name: "gophercolor16x16.png",
                expected_content: ExpectedContent::File("gophercolor16x16.png"),
            },
        ],
        ..Default::default()
    }
);

zip_test_case!(
    "test_prefix",
    ZipTestCase {
        name: "test-prefix.zip",
        comment: Some(b"This is a zipfile comment."),
        files: vec![
            ZipTestFileEntry {
                name: "test.txt",
                expected_content: ExpectedContent::Content(b"This is a test text file.\n".to_vec(),),
            },
            ZipTestFileEntry {
                name: "gophercolor16x16.png",
                expected_content: ExpectedContent::File("gophercolor16x16.png"),
            },
        ],
        ..Default::default()
    }
);

zip_test_case!(
    "symlink",
    ZipTestCase {
        name: "symlink.zip",
        files: vec![ZipTestFileEntry {
            name: "symlink",
            expected_content: ExpectedContent::Content(b"../target".to_vec()),
        }],
        ..Default::default()
    }
);

zip_test_case!(
    "readme",
    ZipTestCase {
        name: "readme.zip",
        ..Default::default()
    }
);

zip_test_case!(
    "winxp",
    ZipTestCase {
        // created in windows XP file manager.
        name: "winxp.zip",
        files: vec![
            ZipTestFileEntry {
                name: "hello",
                expected_content: ExpectedContent::Content(b"world \r\n".to_vec()),
            },
            ZipTestFileEntry {
                name: "dir/bar",
                expected_content: ExpectedContent::Content(b"foo \r\n".to_vec()),
            },
            ZipTestFileEntry {
                name: "dir/empty/",
                expected_content: ExpectedContent::Content(b"".to_vec()),
            },
            ZipTestFileEntry {
                name: "readonly",
                expected_content: ExpectedContent::Content(b"important \r\n".to_vec()),
            },
        ],
        ..Default::default()
    }
);

zip_test_case!(
    "unix",
    ZipTestCase {
        // created by Zip 3.0 under Linux
        name: "unix.zip",
        files: vec![
            ZipTestFileEntry {
                name: "hello",
                expected_content: ExpectedContent::Content(b"world \r\n".to_vec()),
            },
            ZipTestFileEntry {
                name: "dir/bar",
                expected_content: ExpectedContent::Content(b"foo \r\n".to_vec()),
            },
            ZipTestFileEntry {
                name: "dir/empty/",
                expected_content: ExpectedContent::Content(b"".to_vec()),
            },
            ZipTestFileEntry {
                name: "readonly",
                expected_content: ExpectedContent::Content(b"important \r\n".to_vec()),
            },
        ],
        ..Default::default()
    }
);

zip_test_case!(
    "go_with_datadesc_sig",
    ZipTestCase {
        // created by Go, after we wrote the "optional" data
        // descriptor signatures (which are required by macOS)
        name: "go-with-datadesc-sig.zip",
        files: vec![
            ZipTestFileEntry {
                name: "foo.txt",
                expected_content: ExpectedContent::Content(b"foo\n".to_vec()),
            },
            ZipTestFileEntry {
                name: "bar.txt",
                expected_content: ExpectedContent::Content(b"bar\n".to_vec()),
            },
        ],
        ..Default::default()
    }
);

zip_test_case!(
    "crc32_not_streamed",
    ZipTestCase {
        name: "crc32-not-streamed.zip",
        files: vec![
            ZipTestFileEntry {
                name: "foo.txt",
                expected_content: ExpectedContent::Content(b"foo\n".to_vec()),
            },
            ZipTestFileEntry {
                name: "bar.txt",
                expected_content: ExpectedContent::Content(b"bar\n".to_vec()),
            },
        ],
        ..Default::default()
    }
);

zip_test_case!(
    "zip64_2",
    ZipTestCase {
        name: "zip64-2.zip",
        files: vec![ZipTestFileEntry {
            name: "README",
            expected_content: ExpectedContent::Content(
                b"This small file is in ZIP64 format.\n".to_vec(),
            ),
        }],
        ..Default::default()
    }
);

fn process_archive_files<R: rawzip::ReaderAt>(
    archive: &rawzip::ZipArchive<R>,
    case: &ZipTestCase,
    buf: &mut [u8],
) -> Result<(), Error> {
    if let Some(expected_comment_bytes) = case.comment {
        assert_eq!(
            archive.comment().as_bytes(),
            expected_comment_bytes,
            "Comment mismatch for {}",
            case.name
        );
    }

    let mut actual_files_found = 0;

    for expected_file in &case.files {
        let mut found_file = false;
        let mut entries_for_current_expected_file = archive.entries(buf);
        loop {
            match entries_for_current_expected_file.next_entry() {
                Ok(Some(entry)) => {
                    let file_name = entry.file_safe_path().unwrap();

                    if file_name == expected_file.name {
                        actual_files_found += 1;
                        found_file = true;

                        let position = entry.wayfinder();
                        let ent = archive.get_entry(position)?;

                        let mut data = Vec::new();
                        match entry.compression_method() {
                            rawzip::CompressionMethod::Deflate => {
                                let inflater = flate2::read::DeflateDecoder::new(ent.reader());
                                let mut verifier = ent.verifying_reader(inflater);
                                std::io::copy(&mut verifier, &mut Cursor::new(&mut data)).unwrap();
                            }
                            rawzip::CompressionMethod::Store => {
                                let mut verifier = ent.verifying_reader(ent.reader());
                                std::io::copy(&mut verifier, &mut Cursor::new(&mut data)).unwrap();
                            }
                            _ => todo!(
                                "Compression method not yet handled: {:?}",
                                entry.compression_method()
                            ),
                        }

                        match &expected_file.expected_content {
                            ExpectedContent::Content(expected_bytes) => {
                                assert_eq!(
                                    &data, expected_bytes,
                                    "Content mismatch for file {} in {}",
                                    expected_file.name, case.name
                                );
                            }
                            ExpectedContent::File(content_file_name) => {
                                let content_path = Path::new("assets").join(content_file_name);
                                let expected_bytes = std::fs::read(content_path).unwrap();
                                assert_eq!(
                                    &data, &expected_bytes,
                                    "Content mismatch for file {} (from {}) in {}",
                                    expected_file.name, content_file_name, case.name
                                );
                            }
                        }
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => panic!("Error iterating entries in {}: {:?}", case.name, e),
            }
        }
        if !found_file {
            panic!(
                "Expected file {} not found in archive {}",
                expected_file.name, case.name
            );
        }
    }
    assert_eq!(
        actual_files_found,
        case.files.len(),
        "File count mismatch for {}. Expected {}, found {}",
        case.name,
        case.files.len(),
        actual_files_found
    );

    Ok(())
}

fn process_slice_archive_files(
    archive: &rawzip::ZipSliceArchive<&[u8]>,
    case: &ZipTestCase,
) -> Result<(), Error> {
    if let Some(expected_comment_bytes) = case.comment {
        assert_eq!(
            archive.comment().as_bytes(),
            expected_comment_bytes,
            "Comment mismatch for {}",
            case.name
        );
    }

    let mut actual_files_found = 0;

    for expected_file in &case.files {
        let mut found_file = false;
        let mut entries_for_current_expected_file = archive.entries();
        loop {
            match entries_for_current_expected_file.next_entry() {
                Ok(Some(entry)) => {
                    let file_name = entry.file_safe_path().unwrap();

                    if file_name == expected_file.name {
                        actual_files_found += 1;
                        found_file = true;

                        let position = entry.wayfinder();

                        let ent = archive.get_entry(position)?;

                        let mut data = Vec::new();
                        match entry.compression_method() {
                            rawzip::CompressionMethod::Deflate => {
                                let inflater = flate2::read::DeflateDecoder::new(ent.data());
                                let mut verifier = ent.verifying_reader(inflater);
                                std::io::copy(&mut verifier, &mut Cursor::new(&mut data)).unwrap();
                            }
                            rawzip::CompressionMethod::Store => {
                                let mut verifier = ent.verifying_reader(ent.data());
                                std::io::copy(&mut verifier, &mut Cursor::new(&mut data)).unwrap();
                            }
                            _ => todo!(
                                "Compression method not yet handled: {:?}",
                                entry.compression_method()
                            ),
                        }

                        match &expected_file.expected_content {
                            ExpectedContent::Content(expected_bytes) => {
                                assert_eq!(
                                    &data, expected_bytes,
                                    "Content mismatch for file {} in {}",
                                    expected_file.name, case.name
                                );
                            }
                            ExpectedContent::File(content_file_name) => {
                                let content_path = Path::new("assets").join(content_file_name);
                                let expected_bytes = std::fs::read(content_path).unwrap();
                                assert_eq!(
                                    &data, &expected_bytes,
                                    "Content mismatch for file {} (from {}) in {}",
                                    expected_file.name, content_file_name, case.name
                                );
                            }
                        }
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => panic!("Error iterating entries in {}: {:?}", case.name, e),
            }
        }
        if !found_file {
            panic!(
                "Expected file {} not found in archive {}",
                expected_file.name, case.name
            );
        }
    }
    assert_eq!(
        actual_files_found,
        case.files.len(),
        "File count mismatch for {}. Expected {}, found {}",
        case.name,
        case.files.len(),
        actual_files_found
    );

    Ok(())
}

fn run_zip_test_case_reader(case: &ZipTestCase) {
    let file_path = Path::new("assets").join(&case.name);
    let f = File::open(file_path).unwrap();

    fn processor(f: File, case: &ZipTestCase) -> Result<(), Error> {
        let mut buf = vec![0u8; rawzip::RECOMMENDED_BUFFER_SIZE];
        let archive = rawzip::ZipArchive::from_file(f, &mut buf[..])?;
        process_archive_files(&archive, case, &mut buf)?;
        Ok(())
    }

    match (processor(f, case), case.expected_error_kind.as_ref()) {
        (Ok(_), None) => {}
        (Ok(_), Some(expected)) => {
            panic!(
                "Expected error {:?}, but got Ok for {}",
                expected, case.name
            );
        }
        (Err(e), None) => {
            panic!("Unexpected error {:?} for {}", e, case.name);
        }
        (Err(e), Some(expected)) => {
            assert!(
                errors_eq(&e, expected),
                "Error kind mismatch for {}: {:?} != {:?}",
                case.name,
                e.kind(),
                expected
            );
        }
    };
}

fn run_zip_test_case_slice(case: &ZipTestCase) {
    fn processor(case: &ZipTestCase) -> Result<(), Error> {
        let file_path = Path::new("assets").join(&case.name);
        let data = std::fs::read(file_path).unwrap();

        let archive = rawzip::ZipArchive::from_slice(data.as_slice())?;
        process_slice_archive_files(&archive, case)?;
        Ok(())
    }

    match (processor(case), case.expected_error_kind.as_ref()) {
        (Ok(_), None) => {}
        (Ok(_), Some(expected)) => {
            panic!(
                "Expected error {:?}, but got Ok for {}",
                expected, case.name
            );
        }
        (Err(e), None) => {
            panic!("Unexpected error {:?} for {}", e, case.name);
        }
        (Err(e), Some(expected)) => {
            assert!(
                errors_eq(&e, expected),
                "Error kind mismatch for {}: {:?} != {:?}",
                case.name,
                e.kind(),
                expected
            );
        }
    };
}

fn errors_eq(a: &Error, b: &ErrorKind) -> bool {
    match (a.kind(), b) {
        (
            ErrorKind::InvalidSignature {
                expected: a_exp, ..
            },
            ErrorKind::InvalidSignature {
                expected: b_exp, ..
            },
        ) => a_exp == b_exp,
        (
            ErrorKind::InvalidChecksum {
                expected: a_exp, ..
            },
            ErrorKind::InvalidChecksum {
                expected: b_exp, ..
            },
        ) => a_exp == b_exp,
        (
            ErrorKind::InvalidSize {
                expected: a_exp, ..
            },
            ErrorKind::InvalidSize {
                expected: b_exp, ..
            },
        ) => a_exp == b_exp,
        (ErrorKind::InvalidUtf8(a), ErrorKind::InvalidUtf8(b)) => a == b,
        (ErrorKind::InvalidInput { msg: a }, ErrorKind::InvalidInput { msg: b }) => a == b,
        (ErrorKind::IO(a), ErrorKind::IO(b)) => a.kind() == b.kind(),
        (ErrorKind::Eof, ErrorKind::Eof) => true,
        (ErrorKind::MissingEndOfCentralDirectory, ErrorKind::MissingEndOfCentralDirectory) => true,
        (
            ErrorKind::MissingZip64EndOfCentralDirectory,
            ErrorKind::MissingZip64EndOfCentralDirectory,
        ) => true,
        (ErrorKind::BufferTooSmall, ErrorKind::BufferTooSmall) => true,
        _ => false,
    }
}

/// This test is to ensure that the ZipArchive can be created from a Vec<u8>
#[test]
fn zip_integration_tests_vec() {
    let data = std::fs::read("assets/zip64.zip").unwrap();
    let archive = rawzip::ZipArchive::from_slice(data).unwrap();
    assert_eq!(archive.comment().as_bytes(), b"");
    let reader = archive.into_reader();
    let mut buf = vec![0u8; rawzip::RECOMMENDED_BUFFER_SIZE];
    let mut entries = reader.entries(&mut buf);
    let mut count = 0;
    while let Some(entry) = entries.next_entry().unwrap() {
        if entry.is_dir() {
            continue;
        }
        count += 1;
    }
    assert_eq!(count, 1);
}

#[quickcheck]
fn test_read_what_we_write_slice(data: Vec<u8>) {
    let mut output = Vec::new();
    {
        let mut archive = rawzip::ZipArchiveWriter::new(&mut output);
        let mut file = archive
            .new_file("file.txt", rawzip::ZipEntryOptions::default())
            .unwrap();
        let mut writer = rawzip::ZipDataWriter::new(&mut file);
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
