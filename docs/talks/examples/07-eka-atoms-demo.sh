#!/usr/bin/env bash
# Example 6: Eka Atoms Demo
# Shows published atoms and basic Eka concepts

set -e

echo "=== Example 6: Eka Atoms Demo ==="
echo ""
echo "Let's see what Eka atoms look like in practice!"
echo "We'll explore published atoms and show how Eka improves upon Nix."
echo ""

echo "1. Published Atoms Overview"
echo "Press Enter to continue..."
read -r
echo ""

echo "Atoms are published packages that can be discovered and resolved."
echo "Unlike Nix packages (which are tied to nixpkgs), atoms are:"
echo "• Decentralized - can come from any Git repository"
echo "• Versioned - proper semantic versioning support"
echo "• Cryptographically identified - unique BLAKE3 hashes"
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "2. Discovering Published Atoms"
echo "Press Enter to continue..."
read -r
echo ""

echo "Let's look at some published atoms from a real repository:"
echo ""

# Show published atoms from nrdxp's home repo
echo "Published atoms in https://github.com/nrdxp/home:"
echo ""
git ls-remote https://github.com/nrdxp/home 'refs/eka/atoms/*' 2>/dev/null | head -10 || echo "Could not fetch atoms (network issue?)"

echo ""
echo "Each ref like 'refs/eka/atoms/my-atom' represents a published atom!"
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "3. Atom URIs"
echo "Press Enter to continue..."
read -r
echo ""

echo "Atoms are referenced with human-friendly URIs:"
echo ""
echo "Examples:"
echo "• gh:nrdxp/home::network - nrdxp's network config modules"
echo "• ekl:ekapkgs::python3 - Python from ekapkgs"
echo "• https://atoms.example.com::my-lib - From external repo"
echo ""

echo "These resolve to cryptographic IDs for security."
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "4. Manifest Example"
echo "Press Enter to continue..."
read -r
echo ""

cat << 'EOF'
# atom.toml - Declarative dependencies (like Cargo.toml but for atoms)
[package]
name = "my-web-app"
version = "0.1.0"

[package.sets]
company-atoms = "git@github.com:our-company/atoms"
public-atoms = ["https://atoms.example.com", "https://mirror.atoms.example.com"]

[deps.from.company-atoms]
auth-lib = "^2.1"
logging = "^1.0"

[deps.from.public-atoms]
serde = "^1.0"
tokio = "^1.0"
EOF

echo ""
echo "Clean, familiar syntax - no complex Nix expressions!"
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "5. Resolution and Locking"
echo "Press Enter to continue..."
read -r
echo ""

echo "Eka resolves semantic versions to specific commits and generates locks:"
echo ""

cat << 'EOF'
# atom.lock - Cryptographic snapshot (like Cargo.lock but secure)
version = 1

[[deps]]
type = "atom"
label = "serde"
version = "1.0.199"
set = "https://atoms.example.com"
rev = "a1b2c3d4..."  # Exact Git commit
id = "blake3:abc123..."  # Cryptographic atom ID
EOF

echo ""
echo "Same dependency always resolves to same code - guaranteed!"
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "6. Why This Matters"
echo "Press Enter to continue..."
read -r
echo ""

echo "Traditional approaches:"
echo "• npm: Centralized registry, vulnerable to attacks"
echo "• Nix: Powerful but requires Nix language expertise"
echo "• Docker: Good for deployment, bad for development"
echo ""

echo "Eka approach:"
echo "• Decentralized like Git, secure like Nix (but user-friendly)"
echo "• Familiar interface that abstracts Nix complexity"
echo "• Currently uses Nix as backend, but abstracted enough for future backends"
echo "• Package sources stored as dependencies, not just Nix derivations"
echo "• Scales from individual developers to large organizations"
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "7. The Future"
echo "Press Enter to continue..."
read -r
echo ""

echo "Eka enables:"
echo "• Secure software supply chains"
echo "• Decentralized package ecosystems"
echo "• Cross-ecosystem compatibility"
echo "• Developer-friendly reproducible builds"
echo ""

echo "This is the foundation for trustworthy software distribution!"
echo ""

echo "=== End of Demo ==="
echo ""
echo "Eka atoms provide the package management that Nix lacks,"
echo "while maintaining cryptographic security and decentralization."
