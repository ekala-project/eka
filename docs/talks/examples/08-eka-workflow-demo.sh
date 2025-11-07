#!/usr/bin/env bash
# Example 8: Eka Workflow Demo
# Shows the complete Eka workflow from creating atoms to publishing

set -e

echo "=== Example 8: Eka Workflow Demo ==="
echo ""
echo "Let's see the complete Eka workflow in action!"
echo "We'll create an atom, add dependencies, and show the full process."
echo ""

echo "1. Setup"
echo "Press Enter to continue..."
read -r
echo ""

echo "Let's first check that the repository is properly initialized for Eka:"
echo ""

echo "1. Local initialization - ekala.toml file:"
echo ""
cat ../ekala.toml
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "2. Remote initialization - refs/ekala/init:"
echo ""
git ls-remote origin 'refs/ekala/init' 2>/dev/null || echo "Remote init ref not found (expected in demo)"
echo ""

echo "The repository is properly initialized for Eka development!"
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "2. Create a New Atom"
echo "Press Enter to continue..."
read -r
echo ""

echo "Let's create a new atom called 'demo-app':"

# Actually run the command
echo "Running: eka new demo-app"
cargo run -- new demo-app

# Eka creates the directory structure for us
cd demo-app

echo ""
echo "This creates the basic atom structure with atom.toml and atom.lock files."
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "3. Check Initial Files"
echo "Press Enter to continue..."
read -r
echo ""

echo "Let's see what was created:"
echo ""

if [ -f "atom.toml" ]; then
    echo "📄 atom.toml (manifest):"
    head -10 atom.toml
    echo "..."
else
    echo "atom.toml not found - showing example instead:"
    cat << 'EOF'
[package]
name = "demo-app"
version = "0.1.0"

[package.sets]
# Dependency sources would be defined here

[deps.from.company-atoms]
# Dependencies would be listed here
EOF
fi

echo ""

if [ -f "atom.lock" ]; then
    echo "🔒 atom.lock (lockfile):"
    head -10 atom.lock
    echo "..."
else
    echo "atom.lock not found - showing example instead:"
    cat << 'EOF'
version = 1
# Locked dependencies would appear here
# Each with exact versions, commits, and cryptographic IDs
EOF
fi

echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo "4. Add Atom Dependencies"
echo "Press Enter to continue..."
read -r
echo ""

echo "Now let's add some dependencies from published atoms."
echo "We'll add some configuration atoms from nrdxp's home repo."
echo ""

echo "Running: eka add gh:nrdxp/home::dev"
cargo run -- add gh:nrdxp/home::dev

echo ""
echo "Running: eka add gh:nrdxp/home::hosts"
cargo run -- add gh:nrdxp/home::hosts

echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo "5. Add Legacy Nix Dependencies"
echo "Press Enter to continue..."
read -r
echo ""

echo 'Eka can also "pin" traditional Nix fetches'
echo "Let's add nixpkgs as a git dependency:"
echo ""

echo "Running: eka add direct nix pkgs --git nixpkgs-unstable"
cargo run -- add direct nix pkgs --git nixpkgs-unstable

echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo "6. Check Updated Files"
echo "Press Enter to continue..."
read -r
echo ""

echo "Let's see how our files changed after adding dependencies:"
echo ""

if [ -f "atom.toml" ]; then
    echo "📄 Updated atom.toml:"
    cat atom.toml
    echo ""
else
    echo "📄 Example atom.toml with dependencies:"
    cat << 'EOF'
[package]
name = "demo-app"
version = "0.1.0"

[package.sets]
company-atoms = "git@github.com:nrdxp/home"

[deps.from.company-atoms]
serde = "^1.0"
tokio = "^1.0"

[deps.from.direct]
curl = { nix = "github:NixOS/nixpkgs/24.05#legacyPackages.x86_64-linux.curl" }
EOF
fi

if [ -f "atom.lock" ]; then
    echo "🔒 Updated atom.lock:"
    cat atom.lock
    echo ""
else
    echo "🔒 Example atom.lock with resolved dependencies:"
    cat << 'EOF'
version = 1

[[deps]]
type = "atom"
label = "serde"
version = "1.0.199"
set = "git@github.com:nrdxp/home"
rev = "a1b2c3d4..."  # Exact Git commit
id = "blake3:abc123..."  # Cryptographic atom ID

[[deps]]
type = "atom"
label = "tokio"
version = "1.36.0"
set = "git@github.com:nrdxp/home"
rev = "e5f6g7h8..."  # Exact Git commit
id = "blake3:def456..."  # Cryptographic atom ID

[[deps]]
type = "direct"
label = "curl"
nix = "github:NixOS/nixpkgs/24.05#legacyPackages.x86_64-linux.curl"
EOF
fi

echo "Notice how:"
echo "• atom.toml shows our declared dependencies"
echo "• atom.lock shows resolved versions with exact commits and cryptographic IDs"
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "7. Publishing (Demonstration Only)"
echo "Press Enter to continue..."
read -r
echo ""

echo "To publish our atom for others to use, we would run:"
echo ""
echo "eka publish demo-app"
echo ""
echo "This would:"
echo "• Create a Git tag: refs/eka/atoms/demo-app"
echo "• Push the tag to the repository"
echo "• Make the atom available for others to depend on"
echo ""

echo "Publishing requires committing changes, so we won't do it in this demo."
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "8. Using Our Published Atom"
echo "Press Enter to continue..."
read -r
echo ""

echo "Once published, others could depend on our atom like this:"
echo ""
echo "eka add gh:your-username/your-repo::demo-app"
echo ""

echo "This creates a decentralized package ecosystem!"
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "=== End of Eka Workflow Demo ==="
echo ""
echo "We've seen the complete Eka workflow:"
echo "• Create atoms with 'eka new'"
echo "• Add dependencies with 'eka add'"
echo "• Automatic resolution and locking"
echo "• Publish with 'eka publish'"
echo ""
echo "Eka combines familiar package management with Nix's security guarantees!"
echo ""

# Clean up demo atom
cd ..
rm -rf demo-app
