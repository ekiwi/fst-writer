Crate for fuzzing `fst-writer` using [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz/).

## Usage

See [`cargo-fuzz` Usage](https://github.com/rust-fuzz/cargo-fuzz?tab=readme-ov-file#usage) for details.

Here is a list of most common commands, run them from the repository root:

```sh
# run a single fuzzer, until a crash is found.
cargo +nightly fuzz run fstapi_diff

# run a single fuzzer for 1 min (or until a crash is found)
cargo +nightly fuzz run fstapi_diff -- -max_total_time=60

# run many fuzzers in parallel, storing any found crashes but keep going
cargo +nightly fuzz run fstapi_diff -- -fork=$(nproc) -ignore_crashes=1 -max_total_time=3600 --max_len=4096

# generate coverage report
cargo +nightly fuzz coverage fstapi_diff
llvm_cov="$(rustc +nightly --print sysroot)/lib/rustlib/$(rustc +nightly --print host-tuple)/bin/llvm-cov"
$llvm_cov show target/*/coverage/*/release/fstapi_diff \
 --format=html -instr-profile=fuzz/coverage/fstapi_diff/coverage.profdata \
 --output-dir=fuzz/coverage/fstapi_diff/html \
 --ignore-filename-regex="\.cargo|\.rustup|target" \
 --Xdemangler=rustfilt # rustfilt need to be installed separately, you may ommit this line
firefox fuzz/coverage/fstapi_diff/html/index.html
```
