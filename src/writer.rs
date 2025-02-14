use crate::{
    crc, errors::ErrorKind, CompressionMethod, DataDescriptor, Error, ZipFilePath,
    ZipLocalFileHeaderFixed, CENTRAL_HEADER_SIGNATURE, END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES,
};
use std::io::{self, Write};

#[derive(Debug)]
pub struct CountWriter<W> {
    writer: W,
    count: u64,
}

impl<W> CountWriter<W> {
    fn new(writer: W, count: u64) -> Self {
        CountWriter { writer, count }
    }

    fn count(&self) -> u64 {
        self.count
    }
}

impl<W: Write> Write for CountWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let bytes_written = self.writer.write(buf)?;
        self.count += bytes_written as u64;
        Ok(bytes_written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

#[derive(Debug)]
pub struct ZipArchiveWriterBuilder {
    count: u64,
}

impl ZipArchiveWriterBuilder {
    pub fn new() -> Self {
        ZipArchiveWriterBuilder { count: 0 }
    }

    pub fn build<W>(&self, writer: W) -> ZipArchiveWriter<W> {
        ZipArchiveWriter {
            writer: CountWriter::new(writer, self.count),
            files: Vec::new(),
        }
    }
}

impl Default for ZipArchiveWriterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// ```rust
/// use std::io::Write;
///
/// let mut output = std::io::Cursor::new(Vec::new());
/// let mut archive = rawzip::ZipArchiveWriter::new(&mut output);
/// let mut file = archive.new_file("file.txt", rawzip::ZipEntryOptions::default()).unwrap();
/// let mut writer = rawzip::RawZipWriter::new(&mut file);
/// writer.write_all(b"Hello, world!").unwrap();
/// let (_, output) = writer.finish().unwrap();
/// file.finish(output).unwrap();
/// archive.finish().unwrap();
/// ```
#[derive(Debug)]
pub struct ZipArchiveWriter<W> {
    files: Vec<FileHeader>,
    writer: CountWriter<W>,
}

impl ZipArchiveWriter<()> {
    pub fn at_offset(offset: u64) -> ZipArchiveWriterBuilder {
        ZipArchiveWriterBuilder { count: offset }
    }
}

impl<W> ZipArchiveWriter<W> {
    pub fn new(writer: W) -> Self {
        ZipArchiveWriterBuilder::new().build(writer)
    }
}

impl<W> ZipArchiveWriter<W>
where
    W: Write,
{
    pub fn new_dir(&mut self, name: &str) -> Result<(), Error> {
        let file_path = ZipFilePath::new(name.as_bytes());
        if !file_path.is_dir() {
            return Err(Error::from(ErrorKind::InvalidInput(
                "not a directory".to_string(),
            )));
        }

        let safe_file_path = file_path.normalize()?.into_owned();

        if safe_file_path.len() > u16::MAX as usize {
            return Err(Error::from(ErrorKind::InvalidInput(
                "directory name too long".to_string(),
            )));
        }

        let local_header_offset = self.writer.count();
        let flags = 0x0;

        let header = ZipLocalFileHeaderFixed {
            signature: ZipLocalFileHeaderFixed::SIGNATURE,
            version_needed: 20,
            flags,
            compression_method: CompressionMethod::Store.as_id(),
            last_mod_time: 0,
            last_mod_date: 0,
            crc32: 0,
            compressed_size: 0,
            uncompressed_size: 0,
            file_name_len: safe_file_path.len() as u16,
            extra_field_len: 0,
        };

        header.write(&mut self.writer)?;
        self.writer
            .write_all(safe_file_path.as_bytes())
            .map_err(Error::io)?;
        self.files.push(FileHeader {
            name: safe_file_path,
            compression_method: CompressionMethod::Store,
            local_header_offset,
            compressed_size: 0,
            uncompressed_size: 0,
            crc: 0,
        });

        Ok(())
    }

    pub fn new_file<'a>(
        &'a mut self,
        name: &str,
        options: ZipEntryOptions,
    ) -> Result<ZipEntryWriter<'a, W>, Error> {
        let file_path = ZipFilePath::new(name.as_bytes());
        let safe_file_path = file_path.normalize()?.trim_end_matches('/').to_owned();

        if safe_file_path.len() > u16::MAX as usize {
            return Err(Error::from(ErrorKind::InvalidInput(
                "file name too long".to_string(),
            )));
        }

        let local_header_offset = self.writer.count();
        let flags = 0x8; // data descriptor

        let header = ZipLocalFileHeaderFixed {
            signature: ZipLocalFileHeaderFixed::SIGNATURE,
            version_needed: 20,
            flags,
            compression_method: options.compression_method.as_id(),
            last_mod_time: 0,
            last_mod_date: 0,
            crc32: 0,
            compressed_size: 0,
            uncompressed_size: 0,
            file_name_len: safe_file_path.len() as u16,
            extra_field_len: 0,
        };

        header.write(&mut self.writer)?;
        self.writer
            .write_all(safe_file_path.as_bytes())
            .map_err(Error::io)?;

        Ok(ZipEntryWriter::new(
            self,
            safe_file_path,
            local_header_offset,
            options.compression_method,
        ))
    }

    pub fn finish(mut self) -> Result<W, Error>
    where
        W: Write,
    {
        let central_directory_offset = self.writer.count();

        // TODO: zip64
        if self.files.len() > u16::MAX as usize {
            return Err(Error::from(ErrorKind::InvalidInput(
                "too many files".to_string(),
            )));
        }

        let central_directory_entries = self.files.len() as u16;

        for file in self.files {
            // TODO: zip64
            if file.compressed_size >= u32::MAX as u64 || file.uncompressed_size >= u32::MAX as u64
            {
                return Err(Error::from(ErrorKind::InvalidInput(
                    "file too large".to_string(),
                )));
            }

            self.writer
                .write_all(&CENTRAL_HEADER_SIGNATURE.to_le_bytes())
                .map_err(Error::io)?;
            self.writer
                .write_all(&20u16.to_le_bytes())
                .map_err(Error::io)?; // creator version
            self.writer
                .write_all(&20u16.to_le_bytes())
                .map_err(Error::io)?; // reader version
            self.writer
                .write_all(&8u16.to_le_bytes())
                .map_err(Error::io)?; // flags
            self.writer
                .write_all(&file.compression_method.as_id().as_u16().to_le_bytes())
                .map_err(Error::io)?; // method
            self.writer
                .write_all(&0u16.to_le_bytes())
                .map_err(Error::io)?; // modified time
            self.writer
                .write_all(&0u16.to_le_bytes())
                .map_err(Error::io)?; // modified date
            self.writer
                .write_all(&file.crc.to_le_bytes())
                .map_err(Error::io)?; // crc
            self.writer
                .write_all(&(file.compressed_size as u32).to_le_bytes())
                .map_err(Error::io)?; // compressed size
            self.writer
                .write_all(&(file.uncompressed_size as u32).to_le_bytes())
                .map_err(Error::io)?; // uncompressed size

            // todo zip64

            self.writer
                .write_all(&(file.name.len() as u16).to_le_bytes())
                .map_err(Error::io)?; // name length
            self.writer
                .write_all(&0u16.to_le_bytes())
                .map_err(Error::io)?; // extra field length
            self.writer
                .write_all(&0u16.to_le_bytes())
                .map_err(Error::io)?; // file comment length
            self.writer.write_all(&[0u8; 4]).map_err(Error::io)?; // skip disk number start and internal file attr (2x uint16)
            self.writer
                .write_all(&0u32.to_le_bytes())
                .map_err(Error::io)?; // external attrs
            self.writer
                .write_all(&(file.local_header_offset as u32).to_le_bytes())
                .map_err(Error::io)?; // local header offset
            self.writer
                .write_all(file.name.as_bytes())
                .map_err(Error::io)?; // name
                                      // self.writer.write_all(&file.extra_field).map_err(Error::io)?; // extra field
                                      // self.writer.write_all(&file.file_comment).map_err(Error::io)?; // file comment
        }

        let central_directory_end = self.writer.count();
        let central_directory_size = central_directory_end - central_directory_offset;

        // TODO: zip64
        if central_directory_size >= u32::MAX as u64 {
            return Err(Error::from(ErrorKind::InvalidInput(
                "central directory too large".to_string(),
            )));
        }

        self.writer
            .write_all(&END_OF_CENTRAL_DIR_SIGNAUTRE_BYTES)
            .map_err(Error::io)?;
        self.writer.write_all(&[0u8; 4]).map_err(Error::io)?; // skip over disk number and first disk number (2x uint16)
        self.writer
            .write_all(&central_directory_entries.to_le_bytes())
            .map_err(Error::io)?; // number of entries this disk
        self.writer
            .write_all(&central_directory_entries.to_le_bytes())
            .map_err(Error::io)?; // number of entries total
        self.writer
            .write_all(&(central_directory_size as u32).to_le_bytes())
            .map_err(Error::io)?; // size of directory
        self.writer
            .write_all(&(central_directory_offset as u32).to_le_bytes())
            .map_err(Error::io)?; // start of directory
        self.writer
            .write_all(&0u16.to_le_bytes())
            .map_err(Error::io)?; // byte size of EOCD comment

        self.writer.flush().map_err(Error::io)?;
        Ok(self.writer.writer)
    }
}

/// A writer that tracks the number of bytes written in and out.
pub struct ZipEntryWriter<'a, W> {
    inner: &'a mut ZipArchiveWriter<W>,
    compressed_bytes: u64,
    name: String,
    local_header_offset: u64,
    compression_method: CompressionMethod,
}

impl<'a, W> ZipEntryWriter<'a, W> {
    /// Creates a new `TrackingWriter` wrapping the given writer.
    pub(crate) fn new(
        inner: &'a mut ZipArchiveWriter<W>,
        name: String,
        local_header_offset: u64,
        compression_method: CompressionMethod,
    ) -> Self {
        ZipEntryWriter {
            inner,
            compressed_bytes: 0,
            name,
            local_header_offset,
            compression_method,
        }
    }

    /// Returns the total number of bytes successfully written (bytes out).
    pub fn compressed_bytes(&self) -> u64 {
        self.compressed_bytes
    }

    pub fn finish(self, mut output: DataDescriptorOutput) -> Result<(), Error>
    where
        W: Write,
    {
        output.compressed_size = self.compressed_bytes;
        if output.compressed_size >= u32::MAX as u64 || output.uncompressed_size >= u32::MAX as u64
        {
            self.inner
                .writer
                .write_all(&DataDescriptor::SIGNATURE.to_le_bytes())
                .map_err(Error::io)?;
            self.inner
                .writer
                .write_all(&output.crc.to_le_bytes())
                .map_err(Error::io)?;
            self.inner
                .writer
                .write_all(&output.compressed_size.to_le_bytes())
                .map_err(Error::io)?;
            self.inner
                .writer
                .write_all(&output.uncompressed_size.to_le_bytes())
                .map_err(Error::io)?;
        } else {
            self.inner
                .writer
                .write_all(&DataDescriptor::SIGNATURE.to_le_bytes())
                .map_err(Error::io)?;
            self.inner
                .writer
                .write_all(&output.crc.to_le_bytes())
                .map_err(Error::io)?;
            self.inner
                .writer
                .write_all(&(output.compressed_size as u32).to_le_bytes())
                .map_err(Error::io)?;
            self.inner
                .writer
                .write_all(&(output.uncompressed_size as u32).to_le_bytes())
                .map_err(Error::io)?;
        }

        self.inner.files.push(FileHeader {
            name: self.name,
            compression_method: self.compression_method,
            local_header_offset: self.local_header_offset,
            compressed_size: output.compressed_size,
            uncompressed_size: output.uncompressed_size,
            crc: output.crc,
        });

        Ok(())
    }
}

impl<W> Write for ZipEntryWriter<'_, W>
where
    W: Write,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let bytes_written = self.inner.writer.write(buf)?;
        self.compressed_bytes += bytes_written as u64;
        Ok(bytes_written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.writer.flush()
    }
}

#[derive(Debug)]
pub struct RawZipWriter<W> {
    inner: W,
    uncompressed_bytes: u64,
    crc: u32,
}

impl<W> RawZipWriter<W> {
    pub fn new(inner: W) -> Self {
        RawZipWriter {
            inner,
            uncompressed_bytes: 0,
            crc: 0,
        }
    }

    pub fn uncompressed_bytes(&self) -> u64 {
        self.uncompressed_bytes
    }

    pub fn crc(&self) -> u32 {
        self.crc
    }

    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    pub fn finish(mut self) -> Result<(W, DataDescriptorOutput), Error>
    where
        W: Write,
    {
        self.flush().map_err(Error::io)?;
        let output = DataDescriptorOutput {
            crc: self.crc,
            compressed_size: 0,
            uncompressed_size: self.uncompressed_bytes,
        };

        Ok((self.inner, output))
    }
}

impl<W> Write for RawZipWriter<W>
where
    W: Write,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let bytes_written = self.inner.write(buf)?;
        self.uncompressed_bytes += bytes_written as u64;
        self.crc = crc::crc32_chunk(&buf[..bytes_written], self.crc);
        Ok(bytes_written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[derive(Debug)]
pub struct DataDescriptorOutput {
    crc: u32,
    compressed_size: u64,
    uncompressed_size: u64,
}

#[derive(Debug)]
struct FileHeader {
    name: String,
    compression_method: CompressionMethod,
    local_header_offset: u64,
    compressed_size: u64,
    uncompressed_size: u64,
    crc: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct ZipEntryOptions {
    compression_method: CompressionMethod,
}

impl Default for ZipEntryOptions {
    fn default() -> Self {
        ZipEntryOptions {
            compression_method: CompressionMethod::Deflate,
        }
    }
}

impl ZipEntryOptions {
    pub fn compression_method(mut self, compression_method: CompressionMethod) -> Self {
        self.compression_method = compression_method;
        self
    }
}
