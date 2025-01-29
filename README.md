# Rawzip

**🚧 👷 This is pre-alpha prior a v0.1.0. Features are missing. Will eat your homework 👷 🚧**

A low-level Zip archive reader and writer. Pure Rust. Zero dependencies. Zero unsafe. Fast.

## Use Cases

- **Bring your own compressor**: Leverage the best Rust crates for your needs.
- **Concurrent streaming decompression**: Parallelize decompression without synchronization.
- **Minimal allocations**: rawzip deserializes into provided buffers to cut down unnecessary memory usage even as Zip files scale to hundreds of thousands of entries.
- **Zero-copy**: No allocations or copying needed when the entire Zip file is buffered in memory. Ideal for environments like WebAssembly.
- **Raw access**: Directly read the compressed bytes and leverage optimal in-memory libraries like [libdeflater](https://crates.io/crates/libdeflater) and [zune-inflate](https://crates.io/crates/zune-inflate).
- **Zip file creation**: Nothing special, but it's good to be able to create Zip archives.
