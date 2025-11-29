# Configmaton Python Bindings

Python bindings for the configmaton configuration automaton library.

## Setup with UV

### Prerequisites

1. **Build the Rust library:**
   ```bash
   cd ..
   cargo build --release --features cli -p configmaton-ffi
   ```

2. **Copy the library and build the Python extension:**
   ```bash
   cp ../target/release/libconfigmaton_ffi.so configmaton/
   python setup.py build_ext --inplace
   ```

3. **Set up the UV environment:**
   ```bash
   uv sync
   ```

## Usage

The Python bindings accept pre-compiled binary automaton format. Use the CLI tool to convert JSON to binary:

```bash
# Convert JSON configuration to binary automaton
../target/release/configmaton-cli --output automaton.bin < config.json

# Use in Python
cat automaton.bin | python your_script.py
```

### Example

```python
import sys
from configmaton import Configmaton

# Read binary automaton from stdin
binary_data = sys.stdin.buffer.read()

# Create Configmaton instance with command handler
def handle_command(command: bytes):
    print(f"Command: {command.decode()}")

c = Configmaton(binary_data, handle_command)

# Set configuration values
c.set(b"key", b"value")

# Get configuration values
value = c.get(b"key")
```

## Running Tests

```bash
# Generate binary automaton and run test
../target/release/configmaton-cli --output /tmp/simple.bin < tests/simple.json
cat /tmp/simple.bin | uv run python tests/test_basic.py
```

Or use the convenience script:

```bash
./dev.sh
```

## Configuration Format

The JSON configuration defines rules for when to execute commands:

```json
[
    {
        "when": {
            "key1": "regex_pattern",
            "key2": "another_pattern"
        },
        "run": ["command1", "command2"]
    }
]
```

When keys match the patterns, the corresponding commands are executed.

## Development Workflow

After making changes to the Rust code:

```bash
# 1. Rebuild Rust libraries
cd ..
cargo build --release --features cli

# 2. Copy and rebuild Python extension
cd python
cp ../target/release/libconfigmaton_ffi.so configmaton/
python setup.py build_ext --inplace

# 3. Run tests
./dev.sh
```

### Code Formatting

Format Python code with Black (99 character line length):

```bash
# Check formatting
uv run black --check tests/

# Format files
uv run black tests/
```

### Git Hooks

Install pre-commit hooks to automatically check code before committing:

```bash
# Install hooks
uv run pre-commit install

# Run manually on all files
uv run pre-commit run --all-files
```

This will check both Rust and Python code formatting, plus other quality checks.
