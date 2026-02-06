# MVM

The _Maat_ Virtual Machine, a WASM-based zero-knowledge virtual machine (zkVM).

## Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) 1.85 or later (with `rustup`)
- Cargo (comes with Rust)

### Installation

Clone the repository and build the project:

```bash
git clone https://github.com/maatlabs/mvm.git
cd mvm
cargo build --release
```

### Running the Example

To execute the "hello world" WASM binary:

```bash
cargo run --release
```

### Running Tests

Run the full test suite:

```bash
cargo test --all-features
```

### Development

#### Code Formatting

Format code using nightly rustfmt:

```bash
cargo +nightly fmt
```

#### Linting

Run Clippy for linting (zero warnings policy):

```bash
cargo clippy --all-features --all-targets -- -D warnings
```

#### Building Documentation

Generate and view documentation:

```bash
cargo doc --all-features --no-deps --open
```

## Contributing

Thank you for your interest in contributing to this project! All contributions large and small are actively accepted. To get started, please read the [contribution guidelines](#contributing). A good place to start would be [Good First Issues](https://github.com/maatlabs/mvm/labels/good%20first%20issue).

## Acknowledgments

This project's initial implementation is based on the excellent tutorial [Writing a WASM Runtime in Rust](https://skanehira.github.io/writing-a-wasm-runtime-in-rust/) by [skanehira](https://github.com/skanehira). The tutorial and accompanying [tiny-wasm-runtime](https://github.com/skanehira/tiny-wasm-runtime) repository provided an invaluable foundation for understanding WASM runtime internals.

## License

Licensed under either of [Apache License, Version 2.0](./LICENSE-APACHE) or [MIT license](./LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this codebase by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
