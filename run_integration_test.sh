#!/bin/bash
set -e

echo "Running integration test for cross-tool workflows..."

# Run the specific test
cargo test -p loom-integration-tests --test cross_tool_workflow -- --nocapture

echo "Integration test completed."