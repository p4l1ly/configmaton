#!/bin/bash
# Visualize automaton from JSON configuration
# Usage: ./visualize.sh <config.json> [output.svg]

set -e

if [ $# -lt 1 ]; then
    echo "Usage: $0 <config.json> [output.svg]"
    echo ""
    echo "Examples:"
    echo "  $0 python/tests/simple.json"
    echo "  $0 python/tests/kitchen.json kitchen.svg"
    exit 1
fi

INPUT="$1"
OUTPUT="${2:-${INPUT%.json}.svg}"

echo "Generating automaton visualization..."
echo "  Input:  $INPUT"
echo "  Output: $OUTPUT"

./target/release/configmaton-cli --svg "$OUTPUT" < "$INPUT"

echo "✓ Done! View with: xdg-open $OUTPUT"
