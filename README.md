# Rawzip

A low-level Zip archive reader and writer. Pure Rust. Zero dependencies. Zero unsafe. Fast.

## Use Cases

In its current state, rawzip should not be considered a general purpose Zip library like [zip](https://crates.io/crates/zip), [rc-zip](https://crates.io/crates/rc-zip), or [async_zip](https://crates.io/crates/async-zip). Instead, it was born out of a need for the following:

- **Efficiency**: Only pay for what you use. Rawzip does not materialize the central directory when a Zip archive is parsed, and instead provides a lending iterator through the listed Zip entries. For a Zip file with 200k entries, this results in up to 2 orders of magnitude performance increase, as other Zip libraries need 200k+ allocations to rawzip's 0. If storage of all entries is needed for further processing, caller's are able to amortize allocations for arbitrary length fields like file names.

- **Bring your own dependencies**: Rawzip pushes the compression responsibility onto the caller. Rust has a myriad of high quality compression libraries to choose from. For instance, just deflate has a half dozen implementations ([#1](https://crates.io/crates/libdeflater), [#2](https://crates.io/crates/miniz_oxide), [#3](https://crates.io/crates/zune-inflate), [#4](https://crates.io/crates/libz-ng-sys), [#5](https://crates.io/crates/zlib-rs), [#6](https://crates.io/crates/cloudflare-zlib-sys)). This allows Rawzip to reach maturity easier and be passively maintained while letting downstream users pick the exact compressor best suited to their needs. The Zip file specification does not change frequently, and the hope is this library won't either.

## Features:

- Pure Rust. Zero dependencies. Zero unsafe. Fast.
- Facilitates concurrent streaming decompression
- Zero allocation and zero copy when reading from a byte slice
- A simple Zip file writer
