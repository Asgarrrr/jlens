#!/usr/bin/env bash
# Benchmark jlens startup time and peak memory for various fixture sizes.
#
# Prerequisites:
#   - jlens built in release mode: cargo build --release
#   - Fixtures generated: python3 benches/generate_fixtures.py
#   - macOS: uses /usr/bin/time -l for memory measurement
#   - Linux: uses /usr/bin/time -v

set -euo pipefail

JLENS="${1:-target/release/jlens}"
FIXTURES_DIR="benches/fixtures"
RESULTS_FILE="benches/baseline.txt"

if [[ ! -x "$JLENS" ]]; then
    echo "Error: $JLENS not found or not executable."
    echo "Run: cargo build --release"
    exit 1
fi

if [[ ! -d "$FIXTURES_DIR" ]]; then
    echo "Error: $FIXTURES_DIR not found."
    echo "Run: python3 benches/generate_fixtures.py"
    exit 1
fi

# Detect OS for memory measurement
if [[ "$(uname)" == "Darwin" ]]; then
    TIME_CMD="/usr/bin/time -l"
    MEM_GREP="maximum resident set size"
else
    TIME_CMD="/usr/bin/time -v"
    MEM_GREP="Maximum resident set size"
fi

echo "jlens benchmark — $(date)"
echo "Binary: $JLENS"
echo "============================================"
echo ""

{
    echo "# jlens baseline benchmarks — $(date)"
    echo "# Binary: $JLENS"
    echo "# $(uname -a)"
    echo ""

    for fixture in "$FIXTURES_DIR"/*.json; do
        name=$(basename "$fixture")
        size=$(stat -f%z "$fixture" 2>/dev/null || stat --printf="%s" "$fixture" 2>/dev/null)
        size_mb=$(echo "scale=1; $size / 1048576" | bc)

        echo "## $name ($size_mb MB)"

        # Startup time: measure how fast jlens can parse and exit
        # We use --help as a proxy for "can it load?" since the TUI needs a terminal.
        # Instead, we measure parse time by timing the binary with the file, sending 'q' immediately.
        # Use timeout to prevent hangs.

        # Method: time the full startup+quit cycle
        # Send 'q' keystroke after a tiny delay
        start_ns=$(python3 -c "import time; print(int(time.time_ns()))")
        echo "q" | timeout 10 $JLENS "$fixture" >/dev/null 2>&1 || true
        end_ns=$(python3 -c "import time; print(int(time.time_ns()))")
        elapsed_ms=$(( (end_ns - start_ns) / 1000000 ))

        echo "  Startup+quit: ${elapsed_ms}ms"

        # Memory: use /usr/bin/time
        mem_output=$(echo "q" | $TIME_CMD timeout 10 $JLENS "$fixture" 2>&1 >/dev/null || true)
        mem_kb=$(echo "$mem_output" | grep -i "maximum resident" | awk '{print $1}' | tr -d '[:alpha:]' || echo "?")

        echo "  Peak memory: ${mem_kb} (raw from time -l)"
        echo ""
    done
} | tee "$RESULTS_FILE"

echo ""
echo "Results saved to $RESULTS_FILE"
