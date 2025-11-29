#!/bin/bash
set -e

echo "=== Building Rust libraries ==="
cd ..
cargo build --release --features cli

echo ""
echo "=== Copying library and building Python extension ==="
cd python
cp ../target/release/libconfigmaton_ffi.so configmaton/
/usr/bin/python setup.py build_ext --inplace

echo ""
echo "=== Running tests ==="
echo "Test: test_basic.py with simple.json"
../target/release/configmaton-cli --output /tmp/simple.bin < tests/simple.json
cat /tmp/simple.bin | uv run python tests/test_basic.py

echo ""
echo "✓ All done!"
