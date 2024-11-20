# Build

```bash
cargo build --target wasm32-unknown-unknown --release
```

# Run the tests

```bash
# Run all tests
cargo test

# Run only the test_vapi test
cargo test --package ticle_core --test test_vapi -- test_vapi --exact --show-output

# Run only the test_review test
cargo test --package ticle_core --test test_review -- test_review --exact --show-output
```
