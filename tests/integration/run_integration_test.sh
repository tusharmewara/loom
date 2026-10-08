#!/bin/bash
set -e

echo "Running integration tests for cross-tool workflows..."

# Run from the repo root regardless of where the script is invoked from.
cd "$(cd "$(dirname "$0")/../.." && pwd)"
cargo test -p loom-integration-tests --test cross_tool_workflow -- --nocapture 2>&1 | tee integration-test.log

echo "Integration test completed."

# Also run the benchmark script
echo "Running performance benchmarks..."
./crates/loom-integration-tests/tests/run_benchmarks.sh 2>&1 | tee benchmark.log

echo "Benchmarks completed."