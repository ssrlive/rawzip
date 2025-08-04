use crate::errors::{Error, ErrorKind};
use crate::reader_at::{FileReader, ReaderAtExt};
use crate::utils::{le_u16, le_u32, le_u64};
use crate::{
    EndOfCentralDirectory, ReaderAt, Zip64EndOfCentralDirectoryRecord, ZipArchive, ZipSliceArchive,
    ZipString, END_OF_CENTRAL_DIR_LOCATOR_SIGNATURE,
};
use std::cell::RefCell;
use std::fs::File;
use std::io::Seek;

const END_OF_CENTRAL_DIR_SIGNAUTRE: u32 = 0x06054b50;
pub(crate) const END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES: [u8; 4] =
    END_OF_CENTRAL_DIR_SIGNAUTRE.to_le_bytes();

// https://github.com/zlib-ng/minizip-ng/blob/55db144e03027b43263e5ebcb599bf0878ba58de/mz_zip.c#L78
const END_OF_CENTRAL_DIR_MAX_OFFSET: u64 = 1 << 20;

/// Locates the End of Central Directory (EOCD) record in a ZIP archive.
///
/// The `ZipLocator` is responsible for finding the EOCD record, which is crucial
/// for reading the contents of a ZIP file.
pub struct ZipLocator {
    max_search_space: u64,
}

impl Default for ZipLocator {
    fn default() -> Self {
        Self::new()
    }
}

impl ZipLocator {
    /// Creates a new `ZipLocator` with a default maximum search space of 1 MiB
    pub fn new() -> Self {
        ZipLocator {
            max_search_space: END_OF_CENTRAL_DIR_MAX_OFFSET,
        }
    }

    /// Sets the maximum number of bytes to search for the EOCD signature.
    ///
    /// The search is performed backwards from the end of the data source.
    ///
    /// ```rust
    /// use rawzip::ZipLocator;
    ///
    /// let locator = ZipLocator::new().max_search_space(1024 * 64); // 64 KiB
    /// ```
    pub fn max_search_space(mut self, max_search_space: u64) -> Self {
        self.max_search_space = max_search_space;
        self
    }

    fn locate_in_byte_slice(&self, data: &[u8]) -> Result<EndOfCentralDirectory, Error> {
        let location = find_end_of_central_dir_signature(data, self.max_search_space as usize)
            .ok_or(ErrorKind::MissingEndOfCentralDirectory)?;

        let eocd = EndOfCentralDirectoryRecordFixed::parse(&data[location..])?;
        let is_zip64 = eocd.is_zip64();

        if !is_zip64 {
            return Ok(EndOfCentralDirectory {
                zip64: None,
                eocd,
                stream_pos: location as u64,
            });
        }

        let zip64l =
            &data[location.saturating_sub(Zip64EndOfCentralDirectoryLocatorRecord::SIZE)..];
        let zip64_locator = Zip64EndOfCentralDirectoryLocatorRecord::parse(zip64l)?;
        let zip64_eocd = &data[(zip64_locator.directory_offset as usize).min(data.len())..];
        let zip64_record = Zip64EndOfCentralDirectoryRecord::parse(zip64_eocd)?;

        Ok(EndOfCentralDirectory {
            zip64: Some(zip64_record),
            eocd,
            stream_pos: zip64_locator.directory_offset,
        })
    }

    /// Locates the EOCD record within a byte slice.
    ///
    /// On success, returns a `ZipSliceArchive` which allows reading the archive
    /// directly from the slice. On failure, returns the original slice and an `Error`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rawzip::ZipLocator;
    /// use std::fs;
    /// use std::io::Read;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut file = fs::File::open("assets/readme.zip")?;
    /// let mut data = Vec::new();
    /// file.read_to_end(&mut data)?;
    ///
    /// let locator = ZipLocator::new();
    /// match locator.locate_in_slice(&data) {
    ///     Ok(archive) => {
    ///         println!("Found EOCD in slice, archive has {} files.", archive.entries_hint());
    ///     }
    ///     Err((_data, e)) => {
    ///         eprintln!("Failed to locate EOCD in slice: {:?}", e);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn locate_in_slice<T: AsRef<[u8]>>(
        &self,
        data: T,
    ) -> Result<ZipSliceArchive<T>, (T, Error)> {
        match self.locate_in_byte_slice(data.as_ref()) {
            Ok(eocd) => Ok(ZipSliceArchive { data, eocd }),
            Err(e) => Err((data, e)),
        }
    }

    /// Locates the EOCD record within a file.
    ///
    /// A mutable byte slice to use for reading data from the file. The buffer
    /// should be large enough to hold the EOCD record and potentially parts of
    /// the ZIP64 EOCD locator if present. A common size might be a few
    /// kilobytes.
    ///
    /// On failure, returns the original file and an `Error`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rawzip::ZipLocator;
    /// use std::fs::File;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let file = File::open("assets/readme.zip")?;
    /// let mut buffer = vec![0; rawzip::RECOMMENDED_BUFFER_SIZE];
    /// let locator = ZipLocator::new();
    ///
    /// match locator.locate_in_file(file, &mut buffer) {
    ///     Ok(archive) => {
    ///         println!("Found EOCD in file, archive has {} files.", archive.entries_hint());
    ///     }
    ///     Err((_file, e)) => {
    ///         eprintln!("Failed to locate EOCD in file: {:?}", e);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn locate_in_file(
        &self,
        file: std::fs::File,
        buffer: &mut [u8],
    ) -> Result<ZipArchive<FileReader>, (File, Error)> {
        let mut reader = FileReader::from(file);
        let end_offset = match reader.seek(std::io::SeekFrom::End(0)) {
            Ok(offset) => offset,
            Err(e) => return Err((reader.into_inner(), Error::from(e))),
        };
        self.locate_in_reader(reader, buffer, end_offset)
            .map_err(|(fr, e)| (fr.into_inner(), e))
    }

    /// Locates the EOCD record in a reader, treating the specified end offset
    /// as the starting point when searching backwards.
    ///
    /// This method is useful for several scenarios:
    ///
    /// - Zip archive is nowhere near the end of the reader
    /// - Zip archives are concatenated
    ///
    /// For seekable readers, you can determine the end_offset by seeking to the
    /// end of the stream.
    ///
    /// Note that the zip locator may request data passed the end offset in
    /// order to read the entire end of the central directory record + comment.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rawzip::{ZipLocator, FileReader};
    /// use std::fs::File;
    /// use std::io::Seek;
    ///
    /// # fn main() -> Result<(), rawzip::Error> {
    /// let file = File::open("assets/test.zip").unwrap();
    /// let mut reader = FileReader::from(file);
    /// let mut buffer = vec![0; rawzip::RECOMMENDED_BUFFER_SIZE];
    /// let locator = ZipLocator::new();
    ///
    /// // An example of determining the end offset when you don't
    /// // the length but have a seekable reader.
    /// let end_offset = reader.seek(std::io::SeekFrom::End(0)).unwrap();
    /// let archive = locator.locate_in_reader(reader, &mut buffer, end_offset)
    ///     .map_err(|(_, e)| e)?;
    ///
    /// // Maybe there is another zip archive to be found.
    /// // So use the previous archive's start as the end
    /// match locator.locate_in_reader(archive.get_ref(), &mut buffer, archive.base_offset()) {
    ///    Ok(previous_archive) => {
    ///        println!("Found previous ZIP archive!");
    ///    }
    ///    Err((_, _)) => println!("No previous ZIP archive found"),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn locate_in_reader<R>(
        &self,
        mut reader: R,
        buffer: &mut [u8],
        end_offset: u64,
    ) -> Result<ZipArchive<R>, (R, Error)>
    where
        R: ReaderAt,
    {
        let location_result =
            find_end_of_central_dir(&mut reader, buffer, self.max_search_space, end_offset);

        let (stream_pos, buffer_pos, buffer_valid_len) = match location_result {
            Ok(Some(location_tuple)) => location_tuple,
            Ok(None) => {
                return Err((reader, Error::from(ErrorKind::MissingEndOfCentralDirectory)));
            }
            Err(error) => {
                return Err((reader, Error::io(error)));
            }
        };

        // Most likely the single read to find the end of the central directory
        // will fill the buffer with entire end of the central directory (and
        // optionally zip64 end of central directory). So let's try and reuse
        // the the data already in memory as much as possible.
        let reader = Marker::new(reader);

        let mut end_of_central_directory = &buffer[buffer_pos..buffer_valid_len];
        let eocd = loop {
            match EndOfCentralDirectoryRecordFixed::parse(end_of_central_directory) {
                Ok(record) => break record,
                Err(e) if e.is_eof() => {
                    // Unhappy path: the end of central directory crossed over read boundaries
                    let read = reader.read_at_least_at(
                        buffer,
                        EndOfCentralDirectoryRecordFixed::SIZE,
                        stream_pos,
                    );

                    let read = match read {
                        Ok(read) => read,
                        Err(e) => return Err((reader.inner, e)),
                    };

                    end_of_central_directory = &buffer[..read];
                }
                Err(e) => return Err((reader.inner, e)),
            }
        };

        let is_zip64 = eocd.is_zip64();

        end_of_central_directory =
            &end_of_central_directory[EndOfCentralDirectoryRecordFixed::SIZE..];

        let comment_len = eocd.comment_len as usize;
        let mut comment = vec![0u8; comment_len];

        // Unhappy path: entire comment not present in the buffer
        if end_of_central_directory.len() < comment_len {
            comment[..end_of_central_directory.len()].copy_from_slice(end_of_central_directory);
            let pos = end_of_central_directory.len();
            let result = reader.read_exact_at(
                &mut comment[pos..],
                stream_pos + EndOfCentralDirectoryRecordFixed::SIZE as u64 + pos as u64,
            );

            if let Err(e) = result {
                return Err((reader.inner, Error::io(e)));
            }
        } else {
            comment.copy_from_slice(&end_of_central_directory[..comment_len]);
        }

        let comment = ZipString::new(comment);
        if !is_zip64 {
            return Ok(ZipArchive {
                reader: reader.inner,
                comment,
                eocd: EndOfCentralDirectory {
                    zip64: None,
                    eocd,
                    stream_pos,
                },
            });
        }

        let eocd64l_size = Zip64EndOfCentralDirectoryLocatorRecord::SIZE;

        // Unhappy path: if we needed to issue any reads since the original
        // eocd or don't have enough data in the buffer
        let eocd64l_pos = if reader.is_marked() || eocd64l_size > buffer_pos {
            if (eocd64l_size as u64) > stream_pos {
                return Err((
                    reader.inner,
                    Error::from(ErrorKind::MissingZip64EndOfCentralDirectory),
                ));
            }

            let read = reader.read_exact_at(
                &mut buffer[..eocd64l_size],
                stream_pos - eocd64l_size as u64,
            );

            match read {
                Ok(_) => 0,
                Err(e) => return Err((reader.inner, Error::io(e))),
            }
        } else {
            buffer_pos - eocd64l_size
        };

        let zip64l_eocd = &buffer[eocd64l_pos..eocd64l_pos + eocd64l_size];
        let zip64_locator = match Zip64EndOfCentralDirectoryLocatorRecord::parse(zip64l_eocd) {
            Ok(locator) => locator,
            Err(e) => return Err((reader.inner, e)),
        };

        let zip64_eocd_fixed_size = Zip64EndOfCentralDirectoryRecord::SIZE;

        // Unhappy path: zip64 eocd is not in the original buffer
        let (eocd64_start, eocd64_end) = if reader.is_marked()
            || zip64_locator.directory_offset > stream_pos
            || stream_pos - zip64_locator.directory_offset > buffer_pos as u64
        {
            let read = reader.try_read_at_least_at(
                buffer,
                zip64_eocd_fixed_size,
                zip64_locator.directory_offset,
            );

            match read {
                Ok(read) => (0, read),
                Err(e) => {
                    return Err((reader.inner, Error::io(e)));
                }
            }
        } else {
            (
                buffer_pos - (stream_pos - zip64_locator.directory_offset) as usize,
                buffer_valid_len,
            )
        };

        let zip64_eocd = &buffer[eocd64_start..eocd64_end];
        let zip64_record = match Zip64EndOfCentralDirectoryRecord::parse(zip64_eocd) {
            Ok(record) => record,
            Err(e) => return Err((reader.inner, e)),
        };

        // todo: zip64 extensible data sector

        Ok(ZipArchive {
            reader: reader.inner,
            comment,
            eocd: EndOfCentralDirectory {
                zip64: Some(zip64_record),
                eocd,
                stream_pos: zip64_locator.directory_offset,
            },
        })
    }
}

struct Marker<T> {
    inner: T,
    marked: RefCell<bool>,
}

impl<T> Marker<T> {
    fn new(inner: T) -> Self {
        Self {
            inner,
            marked: RefCell::new(false),
        }
    }

    fn is_marked(&self) -> bool {
        *self.marked.borrow()
    }
}

impl<T> ReaderAt for Marker<T>
where
    T: ReaderAt,
{
    fn read_at(&self, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
        match self.inner.read_at(buf, offset) {
            Ok(n) if n > 0 => {
                *self.marked.borrow_mut() = true;
                Ok(n)
            }
            x => x,
        }
    }
}

impl<T> std::io::Seek for Marker<T>
where
    T: std::io::Seek,
{
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct EndOfCentralDirectoryRecordFixed {
    pub(crate) signature: u32,
    pub(crate) disk_number: u16,
    pub(crate) eocd_disk: u16,
    pub(crate) num_entries: u16,
    pub(crate) total_entries: u16,
    pub(crate) central_dir_size: u32,
    pub(crate) central_dir_offset: u32,
    pub(crate) comment_len: u16,
}

impl EndOfCentralDirectoryRecordFixed {
    pub(crate) const SIZE: usize = 22;
    pub fn parse(data: &[u8]) -> Result<EndOfCentralDirectoryRecordFixed, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::from(ErrorKind::Eof));
        }

        let result = EndOfCentralDirectoryRecordFixed {
            signature: le_u32(&data[0..4]),
            disk_number: le_u16(&data[4..6]),
            eocd_disk: le_u16(&data[6..8]),
            num_entries: le_u16(&data[8..10]),
            total_entries: le_u16(&data[10..12]),
            central_dir_size: le_u32(&data[12..16]),
            central_dir_offset: le_u32(&data[16..20]),
            comment_len: le_u16(&data[20..22]),
        };

        if result.signature != END_OF_CENTRAL_DIR_SIGNAUTRE {
            return Err(Error::from(ErrorKind::InvalidSignature {
                expected: END_OF_CENTRAL_DIR_SIGNAUTRE,
                actual: result.signature,
            }));
        }

        Ok(result)
    }

    pub fn is_zip64(&self) -> bool {
        // https://github.com/zlib-ng/minizip-ng/blob/55db144e03027b43263e5ebcb599bf0878ba58de/mz_zip.c#L1011
        self.num_entries == u16::MAX || // 4.4.22
        self.central_dir_offset == u32::MAX // 4.4.24
    }
}

///
///
/// 4.3.15
#[derive(Debug)]
#[allow(dead_code)]
struct Zip64EndOfCentralDirectoryLocatorRecord {
    /// zip64 end of central dir locator signature
    pub signature: u32,

    /// number of the disk with the start of the zip64 end of central directory
    pub eocd_disk: u32,

    /// relative offset of the zip64 end of central directory record
    pub directory_offset: u64,

    /// total number of disks
    pub total_disks: u32,
}

impl Zip64EndOfCentralDirectoryLocatorRecord {
    const SIZE: usize = 20;

    pub fn parse(data: &[u8]) -> Result<Zip64EndOfCentralDirectoryLocatorRecord, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::from(ErrorKind::Eof));
        }

        let result = Zip64EndOfCentralDirectoryLocatorRecord {
            signature: le_u32(&data[0..4]),
            eocd_disk: le_u32(&data[4..8]),
            directory_offset: le_u64(&data[8..16]),
            total_disks: le_u32(&data[16..20]),
        };

        if result.signature != END_OF_CENTRAL_DIR_LOCATOR_SIGNATURE {
            return Err(Error::from(ErrorKind::InvalidSignature {
                expected: END_OF_CENTRAL_DIR_LOCATOR_SIGNATURE,
                actual: result.signature,
            }));
        }

        Ok(result)
    }
}

pub(crate) fn find_end_of_central_dir_signature(
    data: &[u8],
    max_search_space: usize,
) -> Option<usize> {
    let start_search = data.len().saturating_sub(max_search_space);
    backwards_find(
        &data[start_search..],
        &END_OF_CENTRAL_DIR_SIGNAUTRE.to_le_bytes(),
    )
    .map(|pos| pos + start_search)
}

pub(crate) fn find_end_of_central_dir<T>(
    reader: T,
    buffer: &mut [u8],
    max_search_space: u64,
    end_offset: u64,
) -> std::io::Result<Option<(u64, usize, usize)>>
where
    T: ReaderAt,
{
    if buffer.len() < END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES.len() {
        debug_assert!(false, "buffer not big enough to hold signature");
        return Ok(None);
    }

    let max_back = end_offset.saturating_sub(max_search_space);
    let mut offset = end_offset;

    // The amount of data the remains in the stream
    let mut remaining = end_offset - max_back;

    // The number of bytes that were translated from the front to the back
    let mut carry_over = 0;
    loop {
        // We either want to read into the entire buffer (sans the bytes that
        // were carried over from the last read). Or we want to read the remainder
        let read_size = (buffer.len() - carry_over).min(remaining as usize);

        // Need to jump back to the start of the previous read and then how much
        // we want to read
        offset -= read_size as u64;

        // reader.seek_relative(-offset)?;
        reader.read_exact_at(&mut buffer[..read_size], offset)?;
        remaining -= read_size as u64;

        let haystack = &buffer[..read_size + carry_over];
        if let Some(i) = backwards_find(haystack, &END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES) {
            let stream_pos = (max_back + remaining) + (i as u64);
            return Ok(Some((stream_pos, i, read_size + carry_over)));
        }

        if remaining == 0 {
            return Ok(None);
        }

        // Since the signature may be across read boundaries, match how much the
        // end of the signature matches the start of the buffer
        carry_over = match buffer {
            [b0, b1, b2, ..] if [*b0, *b1, *b2] == END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES[1..4] => 3,
            [b0, b1, ..] if [*b0, *b1] == END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES[2..4] => 2,
            [b0, ..] if *b0 == END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES[3] => 1,
            _ => 0,
        };

        if carry_over > 0 {
            // place the carry over bytes at the end of the buffer for the next read
            let dest = (buffer.len() - carry_over).min(remaining as usize);
            buffer.copy_within(..carry_over, dest);
        }
    }
}

fn backwards_find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck_macros::quickcheck;
    use rstest::rstest;
    use std::io::Cursor;

    #[quickcheck]
    fn test_find_end_of_central_dir_signature(mut data: Vec<u8>, offset: usize, chunk_size: u16) {
        if data.len() < 4 {
            return;
        }

        let max_search_space = END_OF_CENTRAL_DIR_MAX_OFFSET;
        let pos = (offset % data.len()).saturating_sub(END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES.len());
        data[pos..pos + 4].copy_from_slice(&END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES);

        let result = find_end_of_central_dir_signature(&data, max_search_space as usize).unwrap();

        let mut buffer = vec![0u8; chunk_size.max(4) as usize];
        let reader = std::io::Cursor::new(&data);
        let (index, buffer_index, buffer_valid_len) =
            find_end_of_central_dir(reader, &mut buffer, max_search_space, data.len() as u64)
                .unwrap()
                .unwrap();

        assert_eq!(index, result as u64);
        assert!(buffer_valid_len > 0, "buffer_valid_len should be positive");
        assert!(
            buffer_valid_len <= buffer.len(),
            "buffer_valid_len should not exceed buffer capacity"
        );
        assert!(
            buffer_index < buffer_valid_len,
            "buffer_index should be within buffer_valid_len"
        );
        assert!(
            buffer_index + END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES.len() <= buffer_valid_len,
            "signature should be within valid part of buffer"
        );
        assert_eq!(
            buffer[buffer_index..buffer_index + 4],
            END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES
        );
    }

    #[quickcheck]
    fn test_find_end_of_central_dir_signature_random(
        data: Vec<u8>,
        chunk_size: u16,
        max_search_space: u64,
    ) {
        let mem = find_end_of_central_dir_signature(&data, max_search_space as usize);

        let mut buffer = vec![0u8; chunk_size.max(4) as usize];
        let reader = std::io::Cursor::new(&data);
        let curse =
            find_end_of_central_dir(reader, &mut buffer, max_search_space, data.len() as u64)
                .unwrap();

        assert_eq!(mem.map(|x| x as u64), curse.map(|(a, _, _)| a));
        if let Some((_, buffer_index, buffer_valid_len)) = curse {
            assert!(buffer_valid_len > 0, "buffer_valid_len should be positive");
            assert!(
                buffer_valid_len <= buffer.len(),
                "buffer_valid_len should not exceed buffer capacity"
            );
            assert!(
                buffer_index < buffer_valid_len,
                "buffer_index should be within buffer_valid_len"
            );
            assert!(
                buffer_index + END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES.len() <= buffer_valid_len,
                "signature should be within valid part of buffer"
            );
        }
    }

    #[rstest]
    #[case(&[], 4, 1000, None)]
    #[case(&[6], 4, 1000, None)]
    #[case(&[5, 6], 4, 1000, None)]
    #[case(&[b'K', 5, 6], 4, 1000, None)]
    #[case(&[0, 6, 0, 0, 0], 4, 1000, None)]
    #[case(&[b'P', b'K', 5, 6], 4, 1000, Some(0))]
    #[case(&[b'P', b'K', 5, 6], 5, 1000, Some(0))]
    #[case(&[b'P', b'K', 5, 6, 5, 6], 5, 1000, Some(0))]
    #[case(&[b'P', b'K', 5, 6, 6, 0, 0, 0], 4, 1000, Some(0))]
    #[case(&[b'P', b'K', 5, 6, 0, 0, 0, 0], 4, 1000, Some(0))]
    #[case(&[b'P', b'K', 5, 6, 0, 0, 0], 4, 1000, Some(0))]
    #[case(&[b'P', b'K', 5, 6, 0], 4, 1000, Some(0))]
    #[case(&[5, 6, b'P', b'K', 5, 6], 4, 1000, Some(2))]
    #[case(&[5, 6, b'P', b'K', 5, 6], 5, 1000, Some(2))]
    #[case(&[5, 6, b'P', b'K', 5, 6, 5, 6], 4, 1000, Some(2))]
    #[case(&[5, 6, b'P', b'K', 5, 6, 5, 6], 5, 1000, Some(2))]
    #[case(&[b'P', b'K', 5, 6, b'P', b'K', 5, 6, 5, 6], 5, 1000, Some(4))]
    #[case(&[b'P', b'K', 5, 6, b'P', b'K', 5, 6, 5, 6], 32, 1000, Some(4))]
    #[case(&[b'P', b'K', 5, 6], 5, 4, Some(0))] // start of max search space tests
    #[case(&[b'P', b'K', 5, 6, 5, 6], 5, 5, None)]
    #[case(&[b'P', b'K', 5, 6, 6, 0, 0, 0], 4, 8, Some(0))]
    #[case(&[b'P', b'K', 5, 6, 0, 0, 0], 4, 8, Some(0))]
    #[case(&[b'P', b'K', 5, 6, 0], 4, 4, None)]
    #[case(&[5, 6, b'P', b'K', 5, 6], 4, 4, Some(2))]
    #[case(&[5, 6, b'P', b'K', 5, 6], 5, 4, Some(2))]
    #[case(&[5, 6, b'P', b'K', 5, 6, 5, 6], 4, 4, None)]
    #[case(&[5, 6, b'P', b'K', 5, 6, 5, 6], 5, 4, None)]
    #[case(&[b'P', b'K', 5, 6, b'P', b'K', 5, 6, 5, 6], 5, 6, Some(4))]
    #[case(&[b'P', b'K', 5, 6, b'P', b'K', 5, 6, 5, 6], 32, 10, Some(4))]
    #[test]
    fn test_find_end_of_central_dir_signature_cases(
        #[case] input: &[u8],
        #[case] buffer_size: usize,
        #[case] max_search_space: u64,
        #[case] expected: Option<u64>,
    ) {
        let result = find_end_of_central_dir_signature(input, max_search_space as usize);
        assert_eq!(result.map(|x| x as u64), expected);

        let cursor = Cursor::new(&input);
        let mut buffer = vec![0u8; buffer_size];
        let found =
            find_end_of_central_dir(cursor, &mut buffer, max_search_space, input.len() as u64)
                .unwrap();
        assert_eq!(found.map(|(a, _, _)| a), expected);

        if expected.is_some() {
            let (_, buffer_pos, buffer_valid_len) = found.unwrap();
            assert!(buffer_valid_len > 0, "buffer_valid_len should be positive");
            assert!(
                buffer_valid_len <= buffer_size,
                "buffer_valid_len should not exceed buffer capacity"
            );
            assert!(
                buffer_pos < buffer_valid_len,
                "buffer_index should be within buffer_valid_len"
            );
            assert!(
                buffer_pos + END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES.len() <= buffer_valid_len,
                "signature should be within valid part of buffer"
            );
            assert_eq!(
                buffer[buffer_pos..buffer_pos + 4],
                END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES
            );
        }
    }
}
