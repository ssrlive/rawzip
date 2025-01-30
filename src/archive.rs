use crate::crc::crc32_chunk;
use crate::errors::{Error, ErrorKind};
use crate::reader_at::{FileReader, MutexReader, ReaderAtExt};
use crate::utils::{le_u16, le_u32, le_u64};
use crate::{EndOfCentralDirectoryRecordFixed, ReaderAt, ZipLocator};
use std::{
    borrow::Cow,
    io::{Read, Seek},
};

pub(crate) const END_OF_CENTRAL_DIR_SIGNATURE64: u32 = 0x06064b50;
pub(crate) const END_OF_CENTRAL_DIR_LOCATOR_SIGNATURE: u32 = 0x07064b50;
pub(crate) const CENTRAL_HEADER_SIGNATURE: u32 = 0x02014b50;
pub(crate) const LOCAL_FILE_HEADER_SIGNATURE: u32 = 0x04034b50;

/// The recommended buffer size to use when reading from a zip file.
///
/// This buffer size was chosen as it can hold an entire central directory
/// record as the spec states (4.4.10):
///
/// > the combined length of any directory and these three fields SHOULD NOT
/// > generally exceed 65,535 bytes.
pub const RECOMMENDED_BUFFER_SIZE: usize = 1 << 16;

pub struct ZipSliceArchive<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) comment: ZipStr<'a>,
    pub(crate) eocd: EndOfCentralDirectory,
}

impl<'a> ZipSliceArchive<'a> {
    pub fn entries(&self) -> ZipSliceEntries<'a> {
        let entry_data = &self.data[(self.eocd.offset() as usize).min(self.data.len())..];
        ZipSliceEntries { entry_data }
    }

    pub fn entries_hint(&self) -> u64 {
        self.eocd.entries()
    }

    pub fn comment(&self) -> ZipStr {
        self.comment
    }

    pub fn get_entry(&self, entry: ZipArchiveEntryWayfinder) -> Result<ZipSliceEntry, Error> {
        let header = &self.data[(entry.local_header_offset as usize).min(self.data.len())..];
        let file_header = ZipLocalFileHeaderFixed::parse(header)?;
        let header = &header[ZipLocalFileHeaderFixed::SIZE..];

        let variable_length = file_header.variable_length();
        let rest = header
            .get(variable_length..)
            .ok_or(Error::from(ErrorKind::Eof))?;

        let (data, rest) = if rest.len() < file_header.compressed_size as usize {
            return Err(Error::from(ErrorKind::Eof));
        } else {
            rest.split_at(file_header.compressed_size as usize)
        };

        let expected_crc = if entry.has_data_descriptor {
            DataDescriptor::parse(rest)?.crc
        } else {
            entry.crc
        };

        Ok(ZipSliceEntry {
            data,
            expected_crc,
            expected_size: entry.uncompressed_size,
        })
    }
}

pub struct ZipSliceEntry<'a> {
    data: &'a [u8],
    expected_crc: u32,
    expected_size: u64,
}

impl<'a> ZipSliceEntry<'a> {
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    pub fn verify_claim(&self, claim: ZipVerification) -> Result<(), Error> {
        if claim.size() != self.expected_size {
            return Err(Error::from(ErrorKind::InvalidSize {
                expected: self.expected_size,
                actual: claim.size(),
            }));
        }

        if self.expected_crc != 0 && self.expected_crc != claim.crc() {
            return Err(Error::from(ErrorKind::InvalidChecksum {
                expected: self.expected_crc,
                actual: claim.crc(),
            }));
        }

        Ok(())
    }
}

pub struct ZipSliceEntries<'data> {
    entry_data: &'data [u8],
}

impl ZipSliceEntries<'_> {
    pub fn next_entry(&mut self) -> Result<Option<ZipFileHeaderRecord>, Error> {
        let Ok(file_header) = ZipFileHeaderFixed::parse(self.entry_data) else {
            return Ok(None);
        };
        self.entry_data = &self.entry_data[ZipFileHeaderFixed::SIZE..];
        let variable_length = file_header.variable_length();
        let entry = ZipFileHeaderRecord::from_parts(file_header, self.entry_data);
        self.entry_data = &self.entry_data[variable_length..];
        Ok(Some(entry))
    }
}

pub struct ZipArchive<R> {
    pub(crate) reader: R,
    pub(crate) comment: ZipString,
    pub(crate) eocd: EndOfCentralDirectory,
}

impl ZipArchive<()> {
    pub fn with_max_search_space(max_search_space: u64) -> ZipLocator {
        ZipLocator::new().max_search_space(max_search_space)
    }

    pub fn from_slice(data: &[u8]) -> Result<ZipSliceArchive, Error> {
        ZipLocator::new().locate_in_slice(data)
    }

    pub fn from_file(
        file: std::fs::File,
        buffer: &mut [u8],
    ) -> Result<ZipArchive<FileReader>, Error> {
        ZipLocator::new().locate_in_file(file, buffer)
    }

    pub fn from_seekable<R>(
        reader: R,
        buffer: &mut [u8],
    ) -> Result<ZipArchive<MutexReader<R>>, Error>
    where
        R: Read + Seek,
    {
        let reader = MutexReader::new(reader);
        ZipLocator::new().locate_in_reader(reader, buffer)
    }
}

impl<R> ZipArchive<R>
where
    R: ReaderAt,
{
    /// Function will seek to and read the central directory, the function
    /// accepts a buffer will be read into and will return borrowed data as long
    /// as the next entry can be read
    pub fn entries<'archive, 'buf>(
        &'archive self,
        buffer: &'buf mut [u8],
    ) -> ZipEntries<'archive, 'buf, R> {
        ZipEntries {
            buffer,
            archive: self,
            entries_yielded: 0,
            pos: 0,
            end: 0,
            offset: self.eocd.offset(),
        }
    }

    pub fn entries_hint(&self) -> u64 {
        self.eocd.entries()
    }

    pub fn comment(&self) -> ZipStr {
        self.comment.as_str()
    }

    pub fn get_entry(&self, entry: ZipArchiveEntryWayfinder) -> Result<ZipEntry<'_, R>, Error> {
        let mut buffer = [0u8; ZipLocalFileHeaderFixed::SIZE];
        self.reader
            .read_exact_at(&mut buffer, entry.local_header_offset)
            .map_err(Error::io)?;

        // The central directory is the source of truth so we really only parse
        // out the local file header to verify the signature and understand the
        // variable length. Not everyone uses this as the source of truth:
        // https://labs.redyops.com/index.php/2020/04/30/spending-a-night-reading-the-zip-file-format-specification/
        let file_header = ZipLocalFileHeaderFixed::parse(&buffer)?;
        let body_offset = entry.local_header_offset
            + ZipLocalFileHeaderFixed::SIZE as u64
            + file_header.variable_length() as u64;

        Ok(ZipEntry {
            archive: self,
            entry,
            body_offset,
            body_end_offset: entry.compressed_size + body_offset,
        })
    }
}

pub struct ZipEntry<'archive, R> {
    archive: &'archive ZipArchive<R>,
    body_offset: u64,
    body_end_offset: u64,
    entry: ZipArchiveEntryWayfinder,
}

impl<'archive, R> ZipEntry<'archive, R>
where
    R: ReaderAt,
{
    pub fn reader(self) -> ZipReader<'archive, R> {
        ZipReader {
            archive: self.archive,
            entry: self.entry,
            offset: self.body_offset,
            end_offset: self.body_end_offset,
        }
    }
}

pub struct ZipVerification {
    pub crc: u32,
    pub uncompressed_size: u64,
}

impl ZipVerification {
    pub fn crc(&self) -> u32 {
        self.crc
    }

    pub fn size(&self) -> u64 {
        self.uncompressed_size
    }
}

/// Verifies the checksum of the decompressed data matches the checksum listed in the zip
#[derive(Debug)]
pub struct ZipVerifier<R> {
    reader: R,
    crc: u32,
    size: u64,
}

impl<R> ZipVerifier<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            crc: 0,
            size: 0,
        }
    }

    pub fn verification_claim(&self) -> ZipVerification {
        ZipVerification {
            crc: self.crc,
            uncompressed_size: self.size,
        }
    }

    pub fn into_inner(self) -> R {
        self.reader
    }
}

impl<R> std::io::Read for ZipVerifier<R>
where
    R: std::io::Read,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.reader.read(buf)?;
        self.crc = crc32_chunk(&buf[..read], self.crc);
        self.size += read as u64;
        Ok(read)
    }
}

pub struct ZipReader<'archive, R> {
    archive: &'archive ZipArchive<R>,
    entry: ZipArchiveEntryWayfinder,
    offset: u64,
    end_offset: u64,
}

impl<R> ZipReader<'_, R>
where
    R: ReaderAt,
{
    pub fn verify_claim(&self, claim: ZipVerification) -> Result<(), Error> {
        if claim.size() != self.entry.uncompressed_size {
            return Err(Error::from(ErrorKind::InvalidSize {
                expected: self.entry.uncompressed_size,
                actual: claim.size(),
            }));
        }

        if self.entry.has_data_descriptor {
            let mut buffer = [0u8; DataDescriptor::SIZE];
            self.archive
                .reader
                .read_exact_at(&mut buffer, self.end_offset)
                .map_err(Error::io)?;
            let descriptor = DataDescriptor::parse(&buffer)?;
            if descriptor.crc != claim.crc() {
                return Err(Error::from(ErrorKind::InvalidChecksum {
                    expected: descriptor.crc,
                    actual: claim.crc(),
                }));
            }
        }

        if self.entry.crc != 0 && self.entry.crc != claim.crc() {
            return Err(Error::from(ErrorKind::InvalidChecksum {
                expected: self.entry.crc,
                actual: claim.crc(),
            }));
        }

        Ok(())
    }
}

impl<R> Read for ZipReader<'_, R>
where
    R: ReaderAt,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read_size = buf.len().min((self.end_offset - self.offset) as usize);
        let read = self
            .archive
            .reader
            .read_at(&mut buf[..read_size], self.offset)?;
        self.offset += read as u64;
        Ok(read)
    }
}

struct DataDescriptor {
    crc: u32,
}

impl DataDescriptor {
    const SIZE: usize = 8;

    fn parse(data: &[u8]) -> Result<DataDescriptor, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::from(ErrorKind::Eof));
        }

        const DATA_DESCRIPTOR_SIGNATURE: u32 = 0x08074b50;
        let mut pos = 0;

        let potential_signature = le_u32(&data[0..4]);
        if potential_signature == DATA_DESCRIPTOR_SIGNATURE {
            pos += 4;
        }

        // The crc is followed by the compressed_size and then the
        // uncompressed_size but the spec allows for the sizes to be either 4
        // bytes each or 8 bytes in Zip64 mode. (spec 4.3.9.1). They aren't
        // needed, so we skip them.
        Ok(DataDescriptor {
            crc: le_u32(&data[pos..pos + 4]),
        })
    }
}

pub(crate) struct EndOfCentralDirectory {
    pub(crate) zip64: Option<Zip64EndOfCentralDirectoryRecord>,
    pub(crate) eocd: EndOfCentralDirectoryRecordFixed,
}

impl EndOfCentralDirectory {
    fn offset(&self) -> u64 {
        self.zip64
            .as_ref()
            .map(|z| z.central_dir_offset)
            .unwrap_or(u64::from(self.eocd.central_dir_offset))
    }

    fn entries(&self) -> u64 {
        self.zip64
            .as_ref()
            .map(|z| z.num_entries)
            .unwrap_or(u64::from(self.eocd.num_entries))
    }
}

pub struct ZipEntries<'archive, 'buf, R> {
    buffer: &'buf mut [u8],
    archive: &'archive ZipArchive<R>,
    entries_yielded: u64,
    pos: usize,
    end: usize,
    offset: u64,
}

impl<R> ZipEntries<'_, '_, R>
where
    R: ReaderAt,
{
    pub fn next_entry(&mut self) -> Result<Option<ZipFileHeaderRecord>, Error> {
        let file_header = loop {
            let data = &self.buffer[self.pos..self.end];
            match ZipFileHeaderFixed::parse(data) {
                Ok(file_header) => break file_header,
                Err(_) if self.entries_yielded == self.archive.entries_hint() => {
                    return Ok(None);
                }
                Err(e) if e.is_eof() => {
                    let remaining = data.len();
                    self.buffer.copy_within(self.pos..self.end, 0);
                    let read = self
                        .archive
                        .reader
                        .try_read_at_least_at(
                            &mut self.buffer[remaining..],
                            ZipFileHeaderFixed::SIZE,
                            self.offset,
                        )
                        .map_err(Error::io)?;
                    self.offset += read as u64;
                    self.pos = 0;
                    self.end = remaining + read;
                    if self.end < ZipFileHeaderFixed::SIZE {
                        return Err(e);
                    }
                }
                Err(e) => return Err(e),
            }
        };

        self.pos += ZipFileHeaderFixed::SIZE;

        let variable_length = file_header.variable_length();

        let remaining = self.end - self.pos;
        if remaining < variable_length {
            self.buffer.copy_within(self.pos..self.end, 0);
            let read = self.archive.reader.read_at_least_at(
                &mut self.buffer[remaining..],
                variable_length - remaining,
                self.offset,
            )?;
            self.offset += read as u64;
            self.pos = 0;
            self.end = remaining + read;
        }

        let file_header = ZipFileHeaderRecord::from_parts(file_header, &self.buffer[self.pos..]);
        self.pos += variable_length;
        self.entries_yielded += 1;
        Ok(Some(file_header))
    }
}

/// 4.4.2
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VersionMadeBy(u16);

#[allow(dead_code)]
impl VersionMadeBy {
    pub fn as_u16(&self) -> u16 {
        self.0
    }

    /// The (major, minor) ZIP specification version supported by the software
    /// used to encode the file.
    ///
    /// 4.4.2.3: The lower byte, The value / 10 indicates the major version
    /// number, and the value mod 10 is the minor version number.
    pub fn version(&self) -> (u8, u8) {
        let v = (self.0 >> 8) as u8;
        (v / 10, v % 10)
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct Zip64EndOfCentralDirectoryRecord {
    /// zip64 end of central dir signature
    pub signature: u32,

    /// size of zip64 end of central directory record
    pub size: u64,

    /// version made by
    pub version_made_by: VersionMadeBy,

    /// version needed to extract
    pub version_needed: u16,

    /// number of this disk
    pub disk_number: u32,

    /// number of the disk with the start of the central directory
    pub cd_disk: u32,

    /// total number of entries in the central directory on this disk
    pub num_entries: u64,

    /// total number of entries in the central directory
    pub total_entries: u64,

    /// size of the central directory
    pub central_dir_size: u64,

    /// offset of start of central directory with respect to the starting disk number
    pub central_dir_offset: u64,
    // zip64 extensible data sector
    // pub extensible_data: Vec<u8>,
}

impl Zip64EndOfCentralDirectoryRecord {
    pub(crate) const SIZE: usize = 56;

    pub fn parse(data: &[u8]) -> Result<Zip64EndOfCentralDirectoryRecord, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::from(ErrorKind::Eof));
        }

        let result = Zip64EndOfCentralDirectoryRecord {
            signature: le_u32(&data[0..4]),
            size: le_u64(&data[4..12]),
            version_made_by: VersionMadeBy(le_u16(&data[12..14])),
            version_needed: le_u16(&data[14..16]),
            disk_number: le_u32(&data[16..20]),
            cd_disk: le_u32(&data[20..24]),
            num_entries: le_u64(&data[24..32]),
            total_entries: le_u64(&data[32..40]),
            central_dir_size: le_u64(&data[40..48]),
            central_dir_offset: le_u64(&data[48..56]),
        };

        if result.signature != END_OF_CENTRAL_DIR_SIGNATURE64 {
            return Err(Error::from(ErrorKind::InvalidSignature {
                expected: END_OF_CENTRAL_DIR_SIGNATURE64,
                actual: result.signature,
            }));
        }

        Ok(result)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompressionMethodId(u16);

impl CompressionMethodId {
    pub fn as_method(&self) -> CompressionMethod {
        match self.0 {
            0 => CompressionMethod::Store,
            1 => CompressionMethod::Shrunk,
            2 => CompressionMethod::Reduce1,
            3 => CompressionMethod::Reduce2,
            4 => CompressionMethod::Reduce3,
            5 => CompressionMethod::Reduce4,
            6 => CompressionMethod::Imploded,
            7 => CompressionMethod::Tokenizing,
            8 => CompressionMethod::Deflate,
            9 => CompressionMethod::Deflate64,
            10 => CompressionMethod::Terse,
            12 => CompressionMethod::Bzip2,
            14 => CompressionMethod::Lzma,
            18 => CompressionMethod::Lz77,
            20 => CompressionMethod::ZstdDeprecated,
            93 => CompressionMethod::Zstd,
            94 => CompressionMethod::Mp3,
            95 => CompressionMethod::Xz,
            96 => CompressionMethod::Jpeg,
            97 => CompressionMethod::WavPack,
            98 => CompressionMethod::Ppmd,
            99 => CompressionMethod::Aes,
            _ => CompressionMethod::Unknown(self.0),
        }
    }
}

/// The compression method used on an individual Zip archive entry
///
/// Documented in the spec under: 4.4.5
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionMethod {
    Store,
    Shrunk,
    Reduce1,
    Reduce2,
    Reduce3,
    Reduce4,
    Imploded,
    Tokenizing,
    Deflate,
    Deflate64,
    Terse,
    Bzip2,
    Lzma,
    Lz77,
    ZstdDeprecated,
    Zstd,
    Mp3,
    Xz,
    Jpeg,
    WavPack,
    Ppmd,
    Aes,
    Unknown(u16),
}

impl From<u16> for CompressionMethod {
    fn from(id: u16) -> Self {
        CompressionMethodId(id).as_method()
    }
}

/// Textual data borrowed from Zip archive
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ZipStr<'a>(&'a [u8]);

impl<'a> ZipStr<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self(data)
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.0
    }
}

/// Textual data from a Zip archive
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ZipString(Vec<u8>);

impl ZipString {
    pub fn new(data: Vec<u8>) -> Self {
        Self(data)
    }

    pub fn as_str(&self) -> ZipStr {
        ZipStr::new(self.0.as_slice())
    }
}

/// Represents a path within a Zip archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ZipFilePath<'a>(ZipStr<'a>);

impl<'a> ZipFilePath<'a> {
    /// Creates a Zip file path from a byte slice.
    pub fn new(data: &'a [u8]) -> Self {
        Self(ZipStr::new(data))
    }

    /// Return the raw bytes of the Zip file path.
    ///
    /// **WARNING**: this may contain be an absolute path or contain a file path
    /// capable of zip slips. Prefer [`normalize`](ZipFilePath::normalize).
    pub fn as_bytes(&self) -> &'a [u8] {
        self.0.as_bytes()
    }

    fn normalize_alloc(s: &str) -> String {
        // 4.4.17.1 All slashes MUST be forward slashes '/'
        let s = s.replace("\\", "/");
        let splits = s.split('/');
        let mut result = String::new();
        for split in splits {
            if split.is_empty() || split == "." {
                continue;
            }

            if split == ".." {
                let last = result.rfind('/');
                result.truncate(last.unwrap_or(0));
                continue;
            }

            if !result.is_empty() {
                result.push('/');
            }

            result.push_str(split);
        }

        result
    }

    /// Represents a path within a Zip archive.
    ///
    /// The path normalization follows these rules:
    /// - Interpret the file path as UTF-8
    /// - Converts backslashes to forward slashes
    /// - Removes redundant slashes
    /// - Resolves relative path components (`..` and `.`)
    /// - Strips leading slashes and parent directory references that would escape the root
    ///
    /// # Examples
    ///
    /// Basic path normalization:
    /// ```
    /// # use rawzip::ZipFilePath;
    /// let path = ZipFilePath::new(b"dir/test.txt");
    /// assert_eq!(path.normalize().unwrap(), "dir/test.txt");
    ///
    /// // Converts backslashes to forward slashes
    /// let path = ZipFilePath::new(b"dir\\test.txt");
    /// assert_eq!(path.normalize().unwrap(), "dir/test.txt");
    ///
    /// // Removes redundant slashes
    /// let path = ZipFilePath::new(b"dir//test.txt");
    /// assert_eq!(path.normalize().unwrap(), "dir/test.txt");
    /// ```
    ///
    /// Handling relative and absolute paths:
    /// ```
    /// # use rawzip::ZipFilePath;
    /// // Removes leading slashes
    /// let path = ZipFilePath::new(b"/test.txt");
    /// assert_eq!(path.normalize().unwrap(), "test.txt");
    ///
    /// // Resolves current directory references
    /// let path = ZipFilePath::new(b"./test.txt");
    /// assert_eq!(path.normalize().unwrap(), "test.txt");
    ///
    /// // Resolves parent directory references
    /// let path = ZipFilePath::new(b"dir/../test.txt");
    /// assert_eq!(path.normalize().unwrap(), "test.txt");
    ///
    /// let path = ZipFilePath::new(b"a/b/c/d/../../test.txt");
    /// assert_eq!(path.normalize().unwrap(), "a/b/test.txt");
    /// ```
    ///
    /// Invalid paths:
    /// ```
    /// # use rawzip::ZipFilePath;
    /// // Invalid UTF-8 sequences result in an error
    /// let path = ZipFilePath::new(&[0xFF]);
    /// assert!(path.normalize().is_err());
    ///
    /// let path = ZipFilePath::new(&[b't', b'e', b's', b't', 0xFF]);
    /// assert!(path.normalize().is_err());
    /// ```
    pub fn normalize(&self) -> Result<Cow<str>, Error> {
        let mut name = std::str::from_utf8(self.as_bytes()).map_err(Error::utf8)?;
        let mut last = 0;
        for &c in name.as_bytes() {
            if matches!(
                (c, last),
                (b'\\', _) | (b'/', b'/') | (b'.', b'.') | (b'.', b'/')
            ) {
                // slow path: intrusive string manipulations required
                return Ok(Cow::Owned(Self::normalize_alloc(name)));
            }
            last = c;
        }

        loop {
            // Fast path: before we trim, do a quick check if they are even necessary.
            name = match name.as_bytes() {
                [b'.', b'.', b'/', ..] => name.trim_start_matches("../"),
                [b'.', b'/', ..] => name.trim_start_matches("./"),
                [b'/', ..] => name.trim_start_matches("/"),
                _ => return Ok(Cow::Borrowed(name)),
            }
        }
    }
}

///
///
/// 4.3.12
#[derive(Debug)]
#[allow(dead_code)]
pub struct ZipFileHeaderRecord<'a> {
    signature: u32,
    version_made_by: u16,
    version_needed: u16,
    flags: u16,
    compression_method: CompressionMethodId,
    last_mod_time: u16,
    last_mod_date: u16,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    file_name_len: u16,
    extra_field_len: u16,
    file_comment_len: u16,
    disk_number_start: u32,
    internal_file_attrs: u16,
    external_file_attrs: u32,
    local_header_offset: u64,
    file_name: ZipFilePath<'a>,
    extra_field: &'a [u8],
    file_comment: ZipStr<'a>,
    is_zip64: bool,
}

impl<'a> ZipFileHeaderRecord<'a> {
    fn from_parts(header: ZipFileHeaderFixed, data: &'a [u8]) -> Self {
        let file_name = &data[..header.file_name_len as usize];
        let data = &data[header.file_name_len as usize..];
        let extra_field = &data[..header.extra_field_len as usize];
        let data = &data[header.extra_field_len as usize..];
        let file_comment = &data[..header.file_comment_len as usize];

        let mut result = Self {
            signature: header.signature,
            version_made_by: header.version_made_by,
            version_needed: header.version_needed,
            flags: header.flags,
            compression_method: header.compression_method,
            last_mod_time: header.last_mod_time,
            last_mod_date: header.last_mod_date,
            crc32: header.crc32,
            compressed_size: u64::from(header.compressed_size),
            uncompressed_size: u64::from(header.uncompressed_size),
            file_name_len: header.file_name_len,
            extra_field_len: header.extra_field_len,
            file_comment_len: header.file_comment_len,
            disk_number_start: u32::from(header.disk_number_start),
            internal_file_attrs: header.internal_file_attrs,
            external_file_attrs: header.external_file_attrs,
            local_header_offset: u64::from(header.local_header_offset),
            file_name: ZipFilePath::new(file_name),
            extra_field,
            file_comment: ZipStr::new(file_comment),
            is_zip64: false,
        };

        if result.uncompressed_size != u64::from(u32::MAX)
            && result.compressed_size != u64::from(u32::MAX)
            && result.local_header_offset != u64::from(u32::MAX)
            && result.disk_number_start != u32::from(u16::MAX)
        {
            return result;
        }

        let mut extra_fields = extra_field;

        loop {
            let Some(kind) = extra_fields.get(0..2).map(le_u16) else {
                break;
            };

            let Some(size) = extra_fields.get(2..4).map(le_u16) else {
                break;
            };

            extra_fields = &extra_fields[4..];
            let end_pos = (size as usize).min(extra_fields.len());
            let (mut field, rest) = extra_fields.split_at(end_pos);
            extra_fields = rest;

            const ZIP64_EXTRA_FIELD: u16 = 0x0001;
            if kind != ZIP64_EXTRA_FIELD {
                continue;
            }

            result.is_zip64 = true;

            if header.uncompressed_size == u32::MAX {
                let Some(uncompressed_size) = field.get(..8).map(le_u64) else {
                    break;
                };
                result.uncompressed_size = uncompressed_size;
                field = &field[8..];
            }

            if header.compressed_size == u32::MAX {
                let Some(compressed_size) = field.get(..8).map(le_u64) else {
                    break;
                };
                result.compressed_size = compressed_size;
                field = &field[8..];
            }

            if header.local_header_offset == u32::MAX {
                let Some(local_header_offset) = field.get(..8).map(le_u64) else {
                    break;
                };
                result.local_header_offset = local_header_offset;
                field = &field[8..];
            }

            if header.disk_number_start == u16::MAX {
                let Some(disk_number_start) = field.get(..4).map(le_u32) else {
                    break;
                };
                result.disk_number_start = disk_number_start;
            }

            break;
        }

        result
    }

    pub fn is_dir(&self) -> bool {
        // Nowhere in the spec does it explicitly say that a directory is identifiable by ending in a slash,
        // but it's a common convention as otherwise it would be an invalid file.
        self.file_name.as_bytes().last() == Some(&b'/')
    }

    /// Describes if the file has a data descriptor that follows the compressed
    /// data
    ///
    /// From the spec (4.3.9.1):
    ///
    /// > This descriptor MUST exist if bit 3 of the general purpose bit flag is
    /// > set
    pub fn has_data_descriptor(&self) -> bool {
        self.flags & 0x08 != 0
    }

    pub fn wayfinder(&self) -> ZipArchiveEntryWayfinder {
        ZipArchiveEntryWayfinder {
            uncompressed_size: self.uncompressed_size,
            compressed_size: self.compressed_size,
            local_header_offset: self.local_header_offset,
            has_data_descriptor: self.has_data_descriptor(),
            crc: self.crc32,
        }
    }

    /// The purported number of bytes of the uncompressed data.
    ///
    /// **WARNING**: this number has not yet been validated, so don't trust it
    /// to make allocation decisions.
    pub fn uncompressed_size_hint(&self) -> u64 {
        self.uncompressed_size
    }

    /// The purported number of bytes of the compressed data.
    ///
    /// **WARNING**: this number has not yet been validated, so don't trust it
    /// to make allocation decisions.
    pub fn compressed_size_hint(&self) -> u64 {
        self.compressed_size
    }

    /// The compression method used to compress the data
    pub fn compression_method(&self) -> CompressionMethod {
        self.compression_method.as_method()
    }

    /// Return the sanitized file path.
    ///
    /// See [`ZipFilePath::normalize`] for more information.
    pub fn file_safe_path(&self) -> Result<Cow<str>, Error> {
        self.file_name.normalize()
    }
}

/// Contains directions to where the Zip entry's data is located within the Zip archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZipArchiveEntryWayfinder {
    uncompressed_size: u64,
    compressed_size: u64,
    local_header_offset: u64,
    crc: u32,
    has_data_descriptor: bool,
}

impl ZipArchiveEntryWayfinder {
    /// Equivalent to [`ZipFileHeaderRecord::compressed_size_hint`]
    ///
    /// This is a convenience method to avoid having to deal with lifetime
    /// issues on a `ZipFileHeaderRecord`
    pub fn uncompressed_size_hint(&self) -> u64 {
        self.uncompressed_size
    }

    /// Equivalent to [`ZipFileHeaderRecord::compressed_size_hint`]
    ///
    /// This is a convenience method to avoid having to deal with lifetime
    /// issues on a `ZipFileHeaderRecord`
    pub fn compressed_size_hint(&self) -> u64 {
        self.compressed_size
    }
}

pub struct ZipLocalFileHeaderFixed {
    pub signature: u32,
    pub version_needed: u16,
    pub flags: u16,
    pub compression_method: u16,
    pub last_mod_time: u16,
    pub last_mod_date: u16,
    pub crc32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub file_name_len: u16,
    pub extra_field_len: u16,
}

impl ZipLocalFileHeaderFixed {
    const SIZE: usize = 30;

    pub fn parse(data: &[u8]) -> Result<ZipLocalFileHeaderFixed, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::from(ErrorKind::Eof));
        }

        let result = ZipLocalFileHeaderFixed {
            signature: le_u32(&data[0..4]),
            version_needed: le_u16(&data[4..6]),
            flags: le_u16(&data[6..8]),
            compression_method: le_u16(&data[8..10]),
            last_mod_time: le_u16(&data[10..12]),
            last_mod_date: le_u16(&data[12..14]),
            crc32: le_u32(&data[14..18]),
            compressed_size: le_u32(&data[18..22]),
            uncompressed_size: le_u32(&data[22..26]),
            file_name_len: le_u16(&data[26..28]),
            extra_field_len: le_u16(&data[28..30]),
        };

        if result.signature != LOCAL_FILE_HEADER_SIGNATURE {
            return Err(Error::from(ErrorKind::InvalidSignature {
                expected: LOCAL_FILE_HEADER_SIGNATURE,
                actual: result.signature,
            }));
        }

        Ok(result)
    }

    pub fn variable_length(&self) -> usize {
        self.file_name_len as usize + self.extra_field_len as usize
    }
}

#[derive(Debug)]
struct ZipFileHeaderFixed {
    pub signature: u32,
    pub version_made_by: u16,
    pub version_needed: u16,
    pub flags: u16,
    pub compression_method: CompressionMethodId,
    pub last_mod_time: u16,
    pub last_mod_date: u16,
    pub crc32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub file_name_len: u16,
    pub extra_field_len: u16,
    pub file_comment_len: u16,
    pub disk_number_start: u16,
    pub internal_file_attrs: u16,
    pub external_file_attrs: u32,
    pub local_header_offset: u32,
}

impl ZipFileHeaderFixed {
    pub fn variable_length(&self) -> usize {
        self.file_name_len as usize + self.extra_field_len as usize + self.file_comment_len as usize
    }
}

impl ZipFileHeaderFixed {
    const SIZE: usize = 46;

    pub fn parse(data: &[u8]) -> Result<ZipFileHeaderFixed, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::from(ErrorKind::Eof));
        }

        let result = ZipFileHeaderFixed {
            signature: le_u32(&data[0..4]),
            version_made_by: le_u16(&data[4..6]),
            version_needed: le_u16(&data[6..8]),
            flags: le_u16(&data[8..10]),
            compression_method: CompressionMethodId(le_u16(&data[10..12])),
            last_mod_time: le_u16(&data[12..14]),
            last_mod_date: le_u16(&data[14..16]),
            crc32: le_u32(&data[16..20]),
            compressed_size: le_u32(&data[20..24]),
            uncompressed_size: le_u32(&data[24..28]),
            file_name_len: le_u16(&data[28..30]),
            extra_field_len: le_u16(&data[30..32]),
            file_comment_len: le_u16(&data[32..34]),
            disk_number_start: le_u16(&data[34..36]),
            internal_file_attrs: le_u16(&data[36..38]),
            external_file_attrs: le_u32(&data[38..42]),
            local_header_offset: le_u32(&data[42..46]),
        };

        if result.signature != CENTRAL_HEADER_SIGNATURE {
            return Err(Error::from(ErrorKind::InvalidSignature {
                expected: CENTRAL_HEADER_SIGNATURE,
                actual: result.signature,
            }));
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(b"test.txt", "test.txt")]
    #[case(b"dir/test.txt", "dir/test.txt")]
    #[case(b"dir\\test.txt", "dir/test.txt")]
    #[case(b"dir//test.txt", "dir/test.txt")]
    #[case(b"/test.txt", "test.txt")]
    #[case(b"../test.txt", "test.txt")]
    #[case(b"dir/../test.txt", "test.txt")]
    #[case(b"./test.txt", "test.txt")]
    #[case(b"dir/./test.txt", "dir/test.txt")]
    #[case(b"dir/./../test.txt", "test.txt")]
    #[case(b"dir/sub/../test.txt", "dir/test.txt")]
    #[case(b"dir/../../test.txt", "test.txt")]
    #[case(b"../../../test.txt", "test.txt")]
    #[case(b"a/b/../../test.txt", "test.txt")]
    #[case(b"a/b/c/../../../test.txt", "test.txt")]
    #[case(b"a/b/c/d/../../test.txt", "a/b/test.txt")]
    fn test_zip_path_normalized(#[case] input: &[u8], #[case] expected: &str) {
        assert_eq!(ZipFilePath::new(input).normalize().unwrap(), expected);
    }

    #[rstest]
    #[case(&[0xFF])]
    #[case(&[b't', b'e', b's', b't', 0xFF])]
    fn test_zip_path_normalized_invalid_utf8(#[case] input: &[u8]) {
        assert!(ZipFilePath::new(input).normalize().is_err());
    }
}
