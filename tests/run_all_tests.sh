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
cargo test --test module_test -- --nocapture

echo ""
echo "Running unit tests..."
cargo test --lib -- --nocapture

echo ""
echo "=========================================="
echo "All tests completed!"
echo "=========================================="

