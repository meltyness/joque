# joque
> [!CAUTION]
> ...

## tests
cargo test
MIRIFLAGS=-Zmiri-ignore-leaks cargo miri test
RUSTFLAGS="-Copt-level=3" cargo test --release  -- --nocapture
