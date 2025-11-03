#!/usr/bin/env bash
# Example 4: Nixpkgs Coupling Pain
# Demonstrates the pain of nixpkgs version management and coupling

set -e

echo "=== Example 4: Nixpkgs Coupling Pain ==="
echo ""
echo "This demonstrates the pain of nixpkgs version management and tight coupling."
echo "We'll show how difficult it is to manage different package versions."
echo ""

# Check if we have nix
if ! command -v nix >/dev/null 2>&1; then
    echo "❌ Nix not found. This demo requires Nix to be installed."
    echo "On NixOS, nix is available by default."
    echo "On other systems, install Nix first."
    exit 1
fi

echo "1. Finding nixpkgs repository..."
echo "Press Enter to continue..."
read -r
echo ""

# Try to find nixpkgs
NIXPKGS_PATH=/var/home/nrd/git/github.com/NixOS/nixpkgs

if [ -n "$NIXPKGS_PATH" ] && [ -d "$NIXPKGS_PATH" ]; then
    echo "Found nixpkgs at: $NIXPKGS_PATH"
    cd "$NIXPKGS_PATH" || exit 1
else
    echo "❌ Could not find nixpkgs. Let's simulate the experience..."
    echo ""
    echo "In a real scenario, you'd need to:"
    echo "1. Find the right nixpkgs commit for your package versions"
    echo "2. Clone or fetch nixpkgs at that specific commit"
    echo "3. Wait for potentially hundreds of MB to download"
    echo "4. Deal with merge conflicts and breaking changes"
    echo ""
    echo "Press Enter to continue..."
    read -r
fi

echo "2. Let's look at package version history..."
echo "Press Enter to continue..."
read -r
echo ""

# Show git log for a common package
if git rev-parse --git-dir > /dev/null 2>&1; then
    echo "Recent commits affecting Python packages:"
    git log --oneline --grep="python" -n 5 2>/dev/null || echo "Could not search git history"
    echo ""
    echo "This shows how frequently packages change!"
    echo ""
else
    echo "Not in a git repository, but imagine seeing:"
    echo "• Commit abc123: python: 3.11.1 -> 3.11.2"
    echo "• Commit def456: pythonPackages.requests: 2.28.1 -> 2.28.2"
    echo "• Commit ghi789: pythonPackages.urllib3: security fix"
    echo ""
    echo "Each commit can break compatibility!"
fi

echo "Press Enter to continue..."
read -r
echo ""

echo "3. The coupling problem: Everything depends on nixpkgs commit"
echo "Press Enter to continue..."
read -r
echo ""

cat << 'EOF'
# Example flake.nix showing the coupling problem
{
  inputs = {
    # Everyone pins to the same nixpkgs commit
    nixpkgs.url = "github:NixOS/nixpkgs/abc123commit";
    
    # But what if you need:
    # - Python 3.10 for project A
    # - Python 3.11 for project B  
    # - Different versions of dependencies?
    
    # You can't easily mix versions!
  };
  
  outputs = { nixpkgs, ... }: {
    # All your packages come from the same nixpkgs snapshot
    packages.default = nixpkgs.python311Packages.buildPythonPackage {
      # ...
    };
  };
}
EOF

echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo "4. Real-world pain: Version conflicts"
echo "Press Enter to continue..."
read -r
echo ""

cat << 'EOF'
# What happens when you need conflicting versions?

# Project A needs: python 3.9 + old requests library
# Project B needs: python 3.11 + new requests library

# In traditional Nix, you might try:
{
  inputs.nixpkgs-old.url = "github:NixOS/nixpkgs/old-commit";
  inputs.nixpkgs-new.url = "github:NixOS/nixpkgs/new-commit";
}

# But now you have:
# - Two copies of nixpkgs (hundreds of MB each)
# - Complex overlay management
# - Maintenance nightmare
# - Still can't easily share packages between versions
EOF

echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo "5. The rebuild cost"
echo "Press Enter to continue..."
read -r
echo ""

echo "Every time nixpkgs updates:"
echo "• Potentially thousands of packages need rebuilding"
echo "• Can take hours or days on slower machines"
echo "• No incremental updates for package subsets"
echo "• All-or-nothing rebuild approach"
echo ""

echo "6. Why this hurts:"
echo ""
echo "• **Slow iteration**: Can't quickly test different versions"
echo "• **Version conflicts**: Hard to use different package versions together"
echo "• **Maintenance burden**: Tracking nixpkgs commits manually"
echo "• **Storage waste**: Duplicate packages for different versions"
echo "• **Coupling**: Everything tied to nixpkgs release cycle"
echo ""

echo "This is why Nix is powerful but painful for complex dependency management!"
echo ""
echo "Eka/Atoms solve this by decoupling packages from repositories and enabling"
echo "proper version resolution - like modern package managers but with reproducibility."

echo ""
echo "=== End of Demo ==="
