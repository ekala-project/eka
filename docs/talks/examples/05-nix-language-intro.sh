#!/usr/bin/env bash
# Example 5: Quick Nix Language Introduction
# Shows a simple Nix expression to give students context

set -e

echo "=== Example 5: Quick Nix Language Introduction ==="
echo ""
echo "Let's look at a simple Nix expression to understand what we're talking about."
echo "This is NOT a deep dive - just enough context for the rest of the talk."
echo ""

# Check if we have nix
if ! command -v nix >/dev/null 2>&1; then
    echo "❌ Nix not found. This demo requires Nix to be installed."
    echo "On NixOS, nix is available by default."
    echo "On other systems, install Nix first."
    echo ""
    echo "We'll show you what Nix expressions look like instead..."
    echo ""
else
    echo "✅ Nix found! Let's see a simple expression."
    echo ""
fi

echo "1. A Simple Nix Expression"
echo "Press Enter to continue..."
read -r
echo ""

cat << 'EOF'
# This is a Nix expression - like JSON but with functions!
{
  # Package metadata
  name = "hello-world";
  version = "1.0.0";
  
  # Build inputs (dependencies)
  buildInputs = [ pkgs.gcc pkgs.make ];
  
  # Build script (what to run)
  buildPhase = ''
    gcc -o hello hello.c
  '';
  
  # Install script
  installPhase = ''
    mkdir -p $out/bin
    cp hello $out/bin/
  '';
}
EOF

echo ""
echo "This defines HOW to build software, not just WHAT to install."
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "2. Key Nix Concepts"
echo "Press Enter to continue..."
read -r
echo ""

echo "• Pure functions: Same inputs always produce same outputs"
echo "• Declarative: You describe WHAT you want, not HOW to get it"
echo "• Immutable: Everything is read-only after creation"
echo "• Coherent: Files stored by their hash, not name"
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "3. Why This Matters"
echo "Press Enter to continue..."
read -r
echo ""

echo "Traditional package managers:"
echo "• 'Install Python 3.9' - ambiguous, changes over time"
echo "• 'Download from pypi.org' - centralized, vulnerable"
echo ""

echo "Nix package managers:"
echo "• 'Build with this exact GCC version and these exact sources'"
echo "• 'Store result at /nix/store/<hash>-python-3.9'"
echo "• Same everywhere, forever"
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "4. The Problem with Nix Expressions"
echo "Press Enter to continue..."
read -r
echo ""

echo "Writing these expressions is HARD:"
echo "• Complex syntax (not just JSON)"
echo "• Deep knowledge required (we skipped a lot)"
echo "• Easy to get wrong"
echo "• Thousands of expressions needed"
echo ""

echo "This is where Eka/Atoms can help - they abstract away the Nix complexity for the end-user."
echo ""

echo "=== End of Demo ==="
echo ""
echo "Now you understand what Nix expressions are - the building blocks"
echo "that make reproducible builds possible, but are too complex for most users."
