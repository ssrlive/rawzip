#[derive(Debug)]
pub struct Error {
    inner: ErrorInner,
}

impl Error {
    pub(crate) fn io(err: std::io::Error) -> Error {
        Error::from(ErrorKind::IO(err))
    }

    pub(crate) fn utf8(err: std::str::Utf8Error) -> Error {
        Error::from(ErrorKind::InvalidUtf8(err))
    }

    pub(crate) fn is_eof(&self) -> bool {
        matches!(self.inner.kind, ErrorKind::Eof)
    }
}

#[derive(Debug)]
struct ErrorInner {
    kind: ErrorKind,
}

#[derive(Debug)]
pub(crate) enum ErrorKind {
    MissingEndOfCentralDirectory,
    MissingZip64EndOfCentralDirectory,
    BufferTooSmall,
    InvalidSignature { expected: u32, actual: u32 },
    InvalidChecksum { expected: u32, actual: u32 },
    InvalidSize { expected: u64, actual: u64 },
    InvalidUtf8(std::str::Utf8Error),
    IO(std::io::Error),
    Eof,
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.inner.kind)?;
        Ok(())
    }
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            ErrorKind::IO(ref err) => err.fmt(f),
            ErrorKind::MissingEndOfCentralDirectory => {
                write!(f, "Missing end of central directory")
            }
            ErrorKind::MissingZip64EndOfCentralDirectory => {
                write!(f, "Missing zip64 end of central directory")
            }
            ErrorKind::BufferTooSmall => {
                write!(f, "Buffer size too small")
            }
            ErrorKind::Eof => {
                write!(f, "Unexpected end of file")
            }
            ErrorKind::InvalidSignature { expected, actual } => {
                write!(
                    f,
                    "Invalid signature: expected 0x{:08x}, got 0x{:08x}",
                    expected, actual
                )
            }
            ErrorKind::InvalidChecksum { expected, actual } => {
                write!(
                    f,
                    "Invalid checksum: expected 0x{:08x}, got 0x{:08x}",
                    expected, actual
                )
            }
            ErrorKind::InvalidSize { expected, actual } => {
                write!(f, "Invalid size: expected {}, got {}", expected, actual)
            }
            ErrorKind::InvalidUtf8(ref err) => {
                write!(f, "Invalid UTF-8: {}", err)
            }
        }
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error {
            inner: ErrorInner { kind },
        }
    }
}

impl From<std::io::Error> for ErrorKind {
    fn from(err: std::io::Error) -> ErrorKind {
        ErrorKind::IO(err)
    }
}
