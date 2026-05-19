#!/bin/bash
set -e

echo "Running integration test for cross-tool workflows..."

# Run the specific test
cargo test --test cross_tool_workflow -- --nocapture

echo "Integration test completed."