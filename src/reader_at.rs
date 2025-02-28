#[cfg(unix)]
use std::os::unix::fs::FileExt;

use crate::errors::{Error, ErrorKind};

/// Provides reading bytes at a specific offset
///
/// This trait is similar to [`std::io::Read`] but with an additional offset
/// parameter that signals where the read should begin offset from the start of
/// the data. This allows methods to not require a mutable reference to the
/// reader, which is critical for zip files to easily offer decompression of
/// multiple files simultaneously without needing to store them in memory.
///
/// This trait is modelled after Go's
/// [`io.ReaderAt`](https://pkg.go.dev/io#ReaderAt) interface, which is used by
/// their own [Zip implementation](https://pkg.go.dev/archive/zip#NewReader).
pub trait ReaderAt {
    /// Read bytes from the reader at a specific offset
    fn read_at(&self, buf: &mut [u8], offset: u64) -> std::io::Result<usize>;

    /// Sibling to [`read_exact`](std::io::Read::read_exact), but at an offset
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
        let mut read = 0;
        while read < buf.len() {
            let latest = self.read_at(&mut buf[read..], offset + (read as u64))?;
            if latest == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "failed to fill whole buffer",
                ));
            }
            read += latest;
        }
        Ok(())
    }
}

pub(crate) trait ReaderAtExt {
    fn try_read_at_least_at(
        &self,
        buffer: &mut [u8],
        size: usize,
        offset: u64,
    ) -> std::io::Result<usize>;

    fn read_at_least_at(&self, buffer: &mut [u8], size: usize, offset: u64)
        -> Result<usize, Error>;

    fn read_at_most_at(
        &self,
        buffer: &mut [u8],
        size: usize,
        offset: u64,
    ) -> std::io::Result<usize>;
}

impl<T: ReaderAt> ReaderAtExt for T {
    fn try_read_at_least_at(
        &self,
        buffer: &mut [u8],
        mut size: usize,
        offset: u64,
    ) -> std::io::Result<usize> {
        size = size.min(buffer.len());
        let mut pos = 0;
        while pos < size {
            let read = self.read_at(&mut buffer[pos..], offset + pos as u64)?;
            if read == 0 {
                return Ok(pos);
            }
            pos += read;
        }
        Ok(pos)
    }

    fn read_at_least_at(
        &self,
        buffer: &mut [u8],
        size: usize,
        offset: u64,
    ) -> Result<usize, Error> {
        if buffer.len() < size {
            return Err(Error::from(ErrorKind::BufferTooSmall));
        }

        let read = self
            .try_read_at_least_at(buffer, size, offset)
            .map_err(Error::io)?;

        if read < size {
            return Err(Error::from(ErrorKind::Eof));
        }

        Ok(read)
    }

    fn read_at_most_at(
        &self,
        buffer: &mut [u8],
        mut size: usize,
        offset: u64,
    ) -> std::io::Result<usize> {
        size = size.min(buffer.len());
        let mut pos = 0;
        while pos < size {
            match self.read_at(&mut buffer[pos..], offset + pos as u64)? {
                0 => break,
                n => pos += n,
            }
        }
        Ok(pos)
    }
}

#[cfg(not(unix))]
pub struct FileReader(MutexReader<std::fs::File>);

/// A file wrapper that implements [`ReaderAt`] across platforms.
#[cfg(unix)]
pub struct FileReader(std::fs::File);

impl FileReader {
    pub fn into_inner(self) -> std::fs::File {
        #[cfg(not(unix))]
        return self.0.into_inner();
        #[cfg(unix)]
        return self.0;
    }
}

impl ReaderAt for FileReader {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
        self.0.read_at(buf, offset)
    }
}

impl std::io::Seek for FileReader {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.0.seek(pos)
    }
}

impl From<std::fs::File> for FileReader {
    #[cfg(not(unix))]
    fn from(file: std::fs::File) -> Self {
        Self(MutexReader(std::sync::Mutex::new(file)))
    }

    #[cfg(unix)]
    fn from(file: std::fs::File) -> Self {
        Self(file)
    }
}

/// A reader that is wrapped in a mutex to allow for concurrent reads.
pub struct MutexReader<R>(std::sync::Mutex<R>);

impl<R> MutexReader<R> {
    pub fn new(inner: R) -> Self {
        Self(std::sync::Mutex::new(inner))
    }

    pub fn into_inner(self) -> R {
        self.0.into_inner().unwrap()
    }
}

impl<R> ReaderAt for MutexReader<R>
where
    R: std::io::Read + std::io::Seek,
{
    /// For seekable implementations, we can emulate the read_at method by
    /// seeking to the offset, reading the data, and then seeking back to the
    /// original position within a mutex.
    ///
    /// This is how Go implements the `io.ReaderAt` interface for filed on
    /// Windows:
    /// https://github.com/golang/go/blob/70b603f4d295573197b43ad090d7cad21895144e/src/internal/poll/fd_windows.go#L525
    fn read_at(&self, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
        let mut lock = self.0.lock().unwrap();
        let original_position = lock.stream_position()?;
        lock.seek(std::io::SeekFrom::Start(offset))?;
        let result = lock.read(buf);
        lock.seek(std::io::SeekFrom::Start(original_position))?;
        result
    }
}

impl<R> std::io::Read for MutexReader<R>
where
    R: std::io::Read,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().read(buf)
    }
}

impl<R> std::io::Seek for MutexReader<R>
where
    R: std::io::Seek,
{
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.0.lock().unwrap().seek(pos)
    }
}

impl<T: ReaderAt> ReaderAt for &'_ T {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
        (*self).read_at(buf, offset)
    }
}

impl<T: ReaderAt> ReaderAt for &'_ mut T {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
        (**self).read_at(buf, offset)
    }
}

impl ReaderAt for &[u8] {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
        let skip = self.len().min(offset as usize);
        let data = &self[skip..];
        let len = data.len().min(buf.len());
        buf[..len].copy_from_slice(&data[..len]);
        Ok(len)
    }
}

impl<R> ReaderAt for std::io::Cursor<R>
where
    R: AsRef<[u8]>,
{
    fn read_at(&self, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
        let data = self.get_ref().as_ref();
        data.read_at(buf, offset)
    }
}
