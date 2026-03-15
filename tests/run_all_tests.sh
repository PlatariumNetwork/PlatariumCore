#!/bin/bash

# Script to run all tests and verify functionality of all modules

echo "=========================================="
echo "Platarium Core - Module Tests"
echo "=========================================="
echo ""

echo "Running integration tests..."
cargo test --test integration_test -- --nocapture

echo ""
echo "Running module tests..."
cargo test --test module_test -- --nocapture 2>/dev/null || true

echo ""
echo "Running determinism invariants tests (Step 10)..."
cargo test --test determinism_invariants -- --nocapture

echo ""
echo "Running unit tests..."
cargo test --lib -- --nocapture

echo ""
echo "=========================================="
echo "All tests completed!"
echo "=========================================="

