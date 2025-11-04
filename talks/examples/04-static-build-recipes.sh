#!/usr/bin/env bash
# Example 3: Static Build Recipes
# Demonstrates how Nix derivations and JSON provide reproducible builds

set -e

echo "=== Example 3: Static Build Recipes ==="
echo ""
echo "This shows how static, declarative build recipes enable reproducible builds."
echo "We'll compare traditional build scripts with Nix-style derivations."
echo ""

echo "1. Traditional Build Script (Unreliable):"
echo "Press Enter to continue..."
read -r
cat << 'EOF'
#!/bin/bash
# build.sh - Traditional build script
gcc -o myprogram main.c -lm
# Problems:
# - Assumes gcc is installed
# - Assumes math library (-lm) exists
# - No version pinning
# - Environment-dependent
EOF
echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo "2. JSON Build Recipe (Better, but still incomplete):"
echo "Press Enter to continue..."
read -r
cat << 'EOF'
{
  "name": "myprogram",
  "version": "1.0.0",
  "build": {
    "compiler": "gcc",
    "flags": ["-o", "myprogram", "main.c", "-lm"],
    "dependencies": ["glibc", "gcc"]
  },
  "inputs": {
    "source": "main.c",
    "headers": []
  }
}
EOF
echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo "3. Nix Derivation (Complete and Reproducible):"
echo ""
echo "Press Enter to continue..."
read -r
echo "Creating a simple Nix derivation..."

# Create a simple C program
cat > example-main.c << 'EOF'
#include <stdio.h>
#include <math.h>

int main() {
    printf("Hello from reproducible build!\n");
    printf("Square root of 16 = %f\n", sqrt(16.0));
    return 0;
}
EOF

# Show what a real Nix derivation looks like
echo "Example Nix derivation (.drv file structure):"
cat << 'EOF'
Derive([("out","/nix/store/abc123...-myprogram-1.0.0","","")],[],[],"
  # Build script embedded in derivation
  mkdir -p $out/bin
  gcc -o $out/bin/myprogram main.c -lm
","x86_64-linux","/nix/store/hash-gcc...-gcc",[
  # Exact inputs with hashes
  ("/nix/store/hash-glibc...-glibc","/nix/store/hash-glibc...-glibc"),
  ("/nix/store/hash-gcc...-gcc","/nix/store/hash-gcc...-gcc"),
  ("main.c","/nix/store/hash-source...-source")
])
EOF
echo ""

echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo "4. Key Differences:"
echo ""
echo "Traditional Script:"
echo "  ❌ Environment-dependent (gcc must be installed)"
echo "  ❌ No version control"
echo "  ❌ Side effects possible"
echo "  ❌ Not declarative"
echo ""

echo "JSON Recipe:"
echo "  ✅ Declarative structure"
echo "  ✅ Lists dependencies"
echo "  ❌ Still environment-dependent"
echo "  ❌ No cryptographic verification"
echo ""

echo "Nix Derivation:"
echo "  ✅ Completely self-contained (hence: *closure*)"
echo "  ✅ Cryptographic hashes for all inputs"
echo "  ✅ Pure functional build"
echo "  ✅ Reproducible across machines/time"
echo "  ✅ Content-addressed outputs"
echo ""

echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo "5. Real Nix Derivation Example:"
echo ""
echo "Let's look at a real derivation from the Nix store:"

# Try to show a real derivation if nix is available
if command -v nix >/dev/null 2>&1; then
    echo "Finding a real derivation..."
    # Look for a simple package derivation
    DERIVATION=$(find /nix/store -name "*.drv" -type f 2>/dev/null | head -1)
    if [ -n "$DERIVATION" ]; then
        echo "Real derivation file: $DERIVATION"
        echo ""
        echo "First few lines:"
        head -10 "$DERIVATION" | cat
        echo ""
        echo "(This shows the exact inputs, outputs, and build script)"
    else
        echo "No derivations found in /nix/store (maybe not on NixOS?)"
    fi
else
    echo "Nix not available, but imagine this file contains:"
    echo "  - Exact hash of every input file"
    echo "  - Exact build commands"
    echo "  - Exact output path (derived from inputs)"
fi

echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo ""
echo "6. Why This Enables Reproducibility:"
echo "• Same inputs → Same derivation hash → Same output path"
echo "• Build is a pure function: inputs → outputs"
echo "• No external state or side effects"
echo "• Verification: re-run build, compare output hash"
echo ""

# Cleanup
rm -f example-main.c

echo "This is how Nix achieves mathematical guarantees of reproducibility!"
