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
