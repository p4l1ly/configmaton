"""
Kitchen Recipe Test - Comprehensive hierarchical configuration example.

This test demonstrates:
1. Hierarchical configuration with nested "then" clauses
2. Recursive configuration updates: commands can update the config state
3. Context-dependent rules: different recipes trigger different ingredient amounts

Key insight: When the automaton emits "set key value" commands, these must be
executed back into the configmaton (cfg.set()) to trigger nested rules.
"""

import subprocess
import os
import pytest
from configmaton import Configmaton


@pytest.fixture
def kitchen_automaton():
    """Compile and load the kitchen automaton."""
    test_dir = os.path.dirname(__file__)
    kitchen_json = os.path.join(test_dir, "kitchen.json")
    cli_path = os.path.join(test_dir, "..", "..", "target", "release", "configmaton-cli")

    # Compile automaton
    result = subprocess.run(
        [cli_path, "--output", "/tmp/test_kitchen.bin"],
        stdin=open(kitchen_json, "rb"),
        capture_output=True,
    )
    assert result.returncode == 0, f"Failed to compile: {result.stderr.decode()}"

    return "/tmp/test_kitchen.bin"


def test_pizza_recipe(kitchen_automaton):
    """Test pizza recipe with nested dough configuration."""
    config_state = {}

    def handle_command(command: bytes):
        """Parse and execute commands from the automaton."""
        cmd = command.decode().strip()
        parts = cmd.split(maxsplit=1)

        if not parts:
            return

        action = parts[0]

        if action == "amount" and len(parts) > 1:
            rest = parts[1].split(maxsplit=1)
            if len(rest) >= 2:
                ingredient = rest[0]
                amount_with_unit = rest[1]
                config_state[f"{ingredient}_amount"] = amount_with_unit

        elif action == "set" and len(parts) > 1:
            rest = parts[1].split(maxsplit=1)
            if len(rest) >= 1:
                key = rest[0]
                value = rest[1] if len(rest) > 1 else ""
                config_state[key] = value
                # Recursively set in configmaton to trigger nested rules
                cfg.set(key.encode(), value.encode())

    with open(kitchen_automaton, "rb") as f:
        cfg = Configmaton(f.read(), handle_command)

    # Set recipe to pizza
    cfg.set(b"recipe", b"pizza")

    # Verify pizza-specific ingredients
    assert config_state.get("sugo_amount") == "100 ml"
    assert config_state.get("mozzarella_amount") == "100 g"
    assert config_state.get("basil_amount") == "10 leaves"
    assert config_state.get("bake_temperature") == "200"
    assert config_state.get("bake_time") == "10 minutes"

    # Now make dough - should trigger nested rules
    cfg.set(b"process", b"dough")
    assert config_state.get("dough_type") == "dry"
    assert config_state.get("flour_type") == "0"

    cfg.set(b"step", b"starter")
    # For dry dough, starter uses 50ml water and 50g flour
    assert config_state.get("water_amount") == "50 ml"
    assert config_state.get("flour_amount") == "50 g"


def test_tomato_soup_recipe(kitchen_automaton):
    """Test tomato soup recipe."""
    config_state = {}

    def handle_command(command: bytes):
        cmd = command.decode().strip()
        parts = cmd.split(maxsplit=1)
        if len(parts) >= 2 and parts[0] == "amount":
            rest = parts[1].split(maxsplit=1)
            if len(rest) >= 2:
                config_state[f"{rest[0]}_amount"] = rest[1]

    with open(kitchen_automaton, "rb") as f:
        cfg = Configmaton(f.read(), handle_command)

    cfg.set(b"recipe", b"tomato soup")

    # Verify tomato soup ingredients
    assert config_state.get("tomato_amount") == "5"
    assert config_state.get("onion_amount") == "1"
    assert config_state.get("garlic_amount") == "1 clove"
    assert config_state.get("salt_amount") == "1 bit"
    assert config_state.get("pepper_amount") == "3 corns"


def test_dumpling_dough(kitchen_automaton):
    """Test dumpling recipe with wet dough."""
    config_state = {}

    def handle_command(command: bytes):
        cmd = command.decode().strip()
        parts = cmd.split(maxsplit=1)

        if not parts:
            return

        if parts[0] == "amount" and len(parts) > 1:
            rest = parts[1].split(maxsplit=1)
            if len(rest) >= 2:
                config_state[f"{rest[0]}_amount"] = rest[1]

        elif parts[0] == "set" and len(parts) > 1:
            rest = parts[1].split(maxsplit=1)
            if len(rest) >= 1:
                key = rest[0]
                value = rest[1] if len(rest) > 1 else ""
                config_state[key] = value
                cfg.set(key.encode(), value.encode())

    with open(kitchen_automaton, "rb") as f:
        cfg = Configmaton(f.read(), handle_command)

    cfg.set(b"recipe", b"dumpling")
    cfg.set(b"process", b"dough")

    # Dumpling uses wet dough
    assert config_state.get("dough_type") == "wet"

    cfg.set(b"step", b"starter")
    # For wet dough, starter uses 100ml water and 100g flour
    assert config_state.get("water_amount") == "100 ml"
    assert config_state.get("flour_amount") == "100 g"


def test_dough_knead_step(kitchen_automaton):
    """Test different flour amounts for knead step."""
    config_state = {}

    def handle_command(command: bytes):
        cmd = command.decode().strip()
        parts = cmd.split(maxsplit=1)

        if not parts:
            return

        if parts[0] == "amount" and len(parts) > 1:
            rest = parts[1].split(maxsplit=1)
            if len(rest) >= 2:
                config_state[f"{rest[0]}_amount"] = rest[1]

        elif parts[0] == "set" and len(parts) > 1:
            rest = parts[1].split(maxsplit=1)
            if len(rest) >= 1:
                key = rest[0]
                value = rest[1] if len(rest) > 1 else ""
                config_state[key] = value
                cfg.set(key.encode(), value.encode())

    with open(kitchen_automaton, "rb") as f:
        cfg = Configmaton(f.read(), handle_command)

    cfg.set(b"recipe", b"pizza")
    cfg.set(b"process", b"dough")
    cfg.set(b"step", b"knead")

    # For dry dough knead, uses 350g flour
    assert config_state.get("flour_amount") == "350 g"
