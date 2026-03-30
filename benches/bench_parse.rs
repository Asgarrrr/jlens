//! Standalone parse benchmark. Compile and run:
//!   rustc --edition 2021 -O benches/bench_parse.rs -o target/bench_parse \
//!     --extern serde_json=$(find target/release/deps -name 'libserde_json-*.rlib' | head -1)
//! Or just use the shell approach below.
//!
//! This file exists as documentation of the measurement methodology.
//! The actual benchmarks are run via bench.sh using the jlens binary.
