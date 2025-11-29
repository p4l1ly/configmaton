"""Basic configmaton tests."""

import subprocess
import os


def test_basic_automaton():
    """Test basic automaton with simple.json configuration."""
    test_dir = os.path.dirname(__file__)
    simple_json = os.path.join(test_dir, "simple.json")
    cli_path = os.path.join(test_dir, "..", "..", "target", "release", "configmaton-cli")

    # Compile automaton
    result = subprocess.run(
        [cli_path, "--output", "/tmp/test_simple.bin"],
        stdin=open(simple_json, "rb"),
        capture_output=True,
    )
    assert result.returncode == 0, f"Failed to compile automaton: {result.stderr.decode()}"

    # Test the automaton
    from configmaton import Configmaton

    commands = []

    def handle_command(cmd: bytes):
        commands.append(cmd.decode())

    with open("/tmp/test_simple.bin", "rb") as f:
        c = Configmaton(f.read(), handle_command)

    # Set foo=bar, qux=ahoy -> should trigger m1
    c.set(b"foo", b"bar")
    c.set(b"qux", b"ahoy")
    assert "m1" in commands, f"Expected 'm1' in commands, got: {commands}"

    # Set foo=baz -> should trigger m2, then m3 and m4 (nested)
    commands.clear()
    c.set(b"foo", b"baz")
    assert "m2" in commands, f"Expected 'm2' in commands, got: {commands}"
    assert "m3" in commands, f"Expected 'm3' in commands, got: {commands}"
    assert "m4" in commands, f"Expected 'm4' in commands, got: {commands}"


def test_get_value():
    """Test getting configuration values."""
    test_dir = os.path.dirname(__file__)
    simple_json = os.path.join(test_dir, "simple.json")
    cli_path = os.path.join(test_dir, "..", "..", "target", "release", "configmaton-cli")

    # Compile automaton
    result = subprocess.run(
        [cli_path, "--output", "/tmp/test_simple2.bin"],
        stdin=open(simple_json, "rb"),
        capture_output=True,
    )
    assert result.returncode == 0

    from configmaton import Configmaton

    with open("/tmp/test_simple2.bin", "rb") as f:
        c = Configmaton(f.read(), lambda x: None)

    # Set and get values
    c.set(b"test_key", b"test_value")
    assert c.get(b"test_key") == b"test_value"

    c.set(b"foo", b"bar")
    assert c.get(b"foo") == b"bar"

    # Non-existent key should return None
    assert c.get(b"nonexistent") is None
