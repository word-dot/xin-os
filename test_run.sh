#!/bin/bash
set -e

echo "=========================================="
echo "Rebuilding and testing rCore-Tutorial-v3"
echo "=========================================="

cd /home/yuxing/rCore-Tutorial-v3

# Clean previous build
echo "Cleaning previous build..."
make clean 2>/dev/null || true

# Build kernel
echo "Building kernel-rv..."
make kernel-rv

# Download image if needed
echo "Checking for test image..."
make download-img-rv

# Run with timeout
echo "=========================================="
echo "Starting QEMU (will timeout after 30s)..."
echo "=========================================="

timeout 30s make run-rv || {
    exit_code=$?
    if [ $exit_code -eq 124 ]; then
        echo ""
        echo "=========================================="
        echo "TIMEOUT: System ran for 30 seconds"
        echo "This might indicate the shell is waiting"
        echo "for input or the system is stuck."
        echo "=========================================="
    else
        echo ""
        echo "=========================================="
        echo "Process exited with code: $exit_code"
        echo "=========================================="
    fi
}

echo ""
echo "Test run completed."