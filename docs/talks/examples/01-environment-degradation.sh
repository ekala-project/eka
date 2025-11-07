#!/usr/bin/env bash
# Example 1: Environment Degradation
# Demonstrates how environments change over time and become unreliable

set -e

echo "=== Example 1: Environment Degradation ==="
echo ""
echo "This demonstrates how environments degrade over time."
echo "We'll simulate installing software and watching how the environment changes."
echo ""

# Create a temporary directory to simulate a "system"
TEMP_DIR=$(mktemp -d)
echo "Created temporary environment: $TEMP_DIR"
echo ""

# Initial state
echo "1. Initial clean environment:"
ls -la "$TEMP_DIR" | head -5
echo ""

# Simulate installing some software (creating files)
echo "2. Installing 'software A' (creates files):"
echo "Press Enter to continue..."
read -r
touch "$TEMP_DIR/software-a-v1.0"
touch "$TEMP_DIR/lib-dependency-1.0"
echo "Installed: software-a-v1.0, lib-dependency-1.0"
ls -la "$TEMP_DIR"
echo ""
echo "Press Enter to continue..."
read -r
echo ""

# Simulate time passing and more installations
echo "3. Time passes... installing 'software B' (conflicts with A):"
echo "Press Enter to continue..."
read -r
touch "$TEMP_DIR/software-b-v2.0"
touch "$TEMP_DIR/lib-dependency-2.0"  # Conflict!
rm "$TEMP_DIR/lib-dependency-1.0"     # Old version removed
echo "Installed: software-b-v2.0, lib-dependency-2.0 (removed old lib-dependency-1.0)"
ls -la "$TEMP_DIR"
echo ""
echo "Press Enter to continue..."
read -r
echo ""

# Simulate system updates and random changes
echo "4. System updates and random changes accumulate:"
echo "Press Enter to continue..."
read -r
touch "$TEMP_DIR/temp-cache-file"
touch "$TEMP_DIR/leftover-config"
chmod +x "$TEMP_DIR/software-a-v1.0"  # Random permission change
echo "Added: temp files, config leftovers, permission changes"
ls -la "$TEMP_DIR"
echo ""
echo "Press Enter to continue..."
read -r
echo ""

# Show the mess
echo "5. Final degraded environment:"
echo "Files: $(ls "$TEMP_DIR" | wc -l) files"
echo "Total size: $(du -sh "$TEMP_DIR" | cut -f1)"
echo "Permissions: $(ls -l "$TEMP_DIR" | grep -c '\-rwxr') executables"
echo ""

echo "=== The Problem ==="
echo "• Software A might not work anymore (dependency removed)"
echo "• Software B might conflict with A"
echo "• Random files and changes accumulate"
echo "• No way to know what changed or when"
echo "• 'Works on my machine' becomes 'Worked yesterday'"
echo ""

# Cleanup
rm -rf "$TEMP_DIR"
echo "Cleaned up temporary environment."