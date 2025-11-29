# Configmaton

A configuration automaton library with Python bindings.

## Development Setup

This project uses [UV](https://github.com/astral-sh/uv) for Python package management.

### Prerequisites

1. Install UV:
```bash
curl -LsSf https://astral.sh/uv/install.sh | sh
```

2. Install Rust (if not already installed):
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Quick Start

```bash
# Build everything and run tests
cd python
./dev.sh
```

### Manual Setup

1. Build the Rust libraries:
```bash
cargo build --release --features cli
```

2. Build the Python extension:
```bash
cd python
cp ../target/release/libconfigmaton_ffi.so configmaton/
python setup.py build_ext --inplace
uv sync
```

### Running Tests

The Python bindings work with pre-compiled binary automaton format:

```bash
# Convert JSON to binary automaton
./target/release/configmaton-cli --output /tmp/simple.bin < python/tests/simple.json

# Run Python test with binary automaton
cd python
cat /tmp/simple.bin | uv run python tests/test_basic.py
```

Expected output:
```
m1
LATER...
m4
m3
m2
```

### Project Structure

- `configmaton/` - Rust core library
- `configmaton-ffi/` - FFI bindings for the Rust library
- `python/` - Python/Cython bindings
  - `configmaton/` - Python package
  - `tests/` - Python test suite

### Development Workflow

1. Make changes to Rust code
2. Format code: `cargo fmt`
3. Rebuild: `cargo build --release --features cli`
4. Rebuild Python extension: `cd python && python setup.py build_ext --inplace`
5. Run tests: `cd python && ./dev.sh`

### Code Formatting

**Rust:** Format with rustfmt (99 character line length)
```bash
# Check formatting
cargo fmt -- --check

# Format all Rust code
cargo fmt
```

**Python:** Format with Black (99 character line length)
```bash
cd python
uv run black --check tests/
uv run black tests/
```

### Git Hooks (Pre-commit)

Install git hooks to automatically format code before committing:

```bash
cd python
uv run pre-commit install
```

The hooks will run automatically on `git commit`. To run manually:

```bash
cd python
# Run on all files
uv run pre-commit run --all-files

# Run on staged files only
uv run pre-commit run
```

Hooks include:
- Rust formatting (`cargo fmt`)
- Rust compilation check (`cargo check`)
- Python formatting (Black)
- Trailing whitespace, end-of-file fixes
- YAML/TOML validation
