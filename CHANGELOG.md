## v0.2.0 - May 26th, 2025

- Expose `ErrorKind` as part of public API
- Impl Iterator for ZipSliceEntries
- Remove accidental types as part of public API
- Derive `Debug` for `MutexReader`
- Fix slice archive panic on truncated eocd entry
- Fix inconsistent behavior for truncated eocd64 between reader and slice archives
- Fix inconsistent behavior for truncated zip comments between reader and slice archives
- Fix zip archive reader reading zeros instead of EOF
- Fix zip archive reader comment detection

## v0.1.0 - March 1th, 2025

The v0.1.0 release signifies that I'm satisfied with overall APIs for reading and writing. There are still plenty of missing aspects that would be useful for a general purpose zip library reader (like timestamps, permissions, etc) as well as writer (zip64), but these can be incorporated onto the current foundations as time and use cases permits.

- Add `ZipSliceArchive::as_bytes` to get access to the underlying input byte stream
- Add `ZipSliceEntry::claim_verifier`
- Change `ZipLocator::locate_in_slice` to return input ownership when there is an error
- Change `ZipSliceArchive` to be generic over any type that implements `AsRef<&[u8]>`
- Rename `RawZipWriter` to `ZipDataWriter`
- Rename `ZipSliceArchive::into_owned` to `into_reader`

## v0.0.7 - February 18th, 2025

- Update `ZipSliceArchive` to pull compressed data size from central directory instead of local file header

## v0.0.6 - February 14th, 2025

- Add the ability to create Zip files

## v0.0.5 - February 11th, 2025

- Improved support for zips with arbitrary leading data
- Expose base offset of where the zip file begins proper
- Expose inner ReaderAt with `get_ref`

## v0.0.4 - February 8th, 2025

- Add exposure of file local header offset
- Add `Debug` and `Clone` implementations to most structs
- Add `ReaderError` to `ZipLocator` to return file ownership back to caller on failure

## v0.0.3 - February 6th, 2025

- Add an `into_owned` to transform a `ZipSliceArchive` into a `ZipArchive`
- Standardize on "verifying_reader" nomenclature

## v0.0.2 - February 1st, 2025

- Update zip verification API
- Update safe file path to remove drive letters
- ZipVerifier to automatically verify at end of stream
- Expose additional types

## v0.0.1 - January 30th, 2025

- Initial workable, pre-alpha release
