#!/usr/bin/env bash
# Example 2: Merkle Tree Demonstration
# Shows how cryptographic hashes enable data integrity verification

set -e

echo "=== Example 2: Merkle Tree Demo ==="
echo ""
echo "This demonstrates how cryptographic hashes work and how Merkle trees"
echo "enable efficient verification of data integrity."
echo ""

# Function to hash data
hash_data() {
    echo -n "$1" | openssl dgst -sha256 -binary | xxd -p -c 32
}

echo "1. Basic Cryptographic Hashing:"
echo "Data: 'hello'"
DATA1="hello"
HASH1=$(hash_data "$DATA1")
echo "Data: '$DATA1' → SHA256: $HASH1"
echo ""

echo "Data: 'hello world'"
DATA2="hello world"
HASH2=$(hash_data "$DATA2")
echo "Data: '$DATA2' → SHA256: $HASH2"
echo ""

echo "Notice: Even adding ' world' completely changes the hash!"
echo "Press Enter to continue..."
read -r
echo ""

echo "2. Building a Simple Merkle Tree:"
echo ""
echo "Press Enter to continue..."
read -r
echo ""

# Create some sample data (like file contents or transactions)
DATA1="file1.txt: Hello World"
DATA2="file2.txt: Foo Bar"
DATA3="file3.txt: Baz Qux"
DATA4="file4.txt: Last File"

echo "Leaf nodes (individual files):"
LEAF1=$(hash_data "$DATA1")
LEAF2=$(hash_data "$DATA2")
LEAF3=$(hash_data "$DATA3")
LEAF4=$(hash_data "$DATA4")

echo "Data: '$DATA1' → Hash: $LEAF1"
echo "Data: '$DATA2' → Hash: $LEAF2"
echo "Data: '$DATA3' → Hash: $LEAF3"
echo "Data: '$DATA4' → Hash: $LEAF4"
echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo "Branch nodes (combine child hashes):"
BRANCH1=$(hash_data "${LEAF1}${LEAF2}")
BRANCH2=$(hash_data "${LEAF3}${LEAF4}")

echo "Branch 1 (hash of File1 + File2): $BRANCH1"
echo "Branch 2 (hash of File3 + File4): $BRANCH2"
echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo "Root node (Merkle root):"
ROOT=$(hash_data "${BRANCH1}${BRANCH2}")
echo "Merkle Root (hash of Branch1 + Branch2): $ROOT"
echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo "3. Verification: Change one file and see the root change"
echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo "Changing file 3 from '$DATA3' to 'file3.txt: MODIFIED'"
DATA3_MODIFIED="file3.txt: MODIFIED"

LEAF3_MODIFIED=$(hash_data "$DATA3_MODIFIED")
echo "New data: '$DATA3_MODIFIED' → New File 3 hash: $LEAF3_MODIFIED"
echo ""
echo "Press Enter to continue..."
read -r
echo ""

BRANCH2_MODIFIED=$(hash_data "${LEAF3_MODIFIED}${LEAF4}")
echo "New Branch 2 (hash of modified File3 + File4): $BRANCH2_MODIFIED"
echo ""
echo "Press Enter to continue..."
read -r
echo ""

ROOT_MODIFIED=$(hash_data "${BRANCH1}${BRANCH2_MODIFIED}")
echo "New Merkle Root (hash of Branch1 + new Branch2): $ROOT_MODIFIED"
echo ""

echo "Original root: $ROOT"
echo "Modified root: $ROOT_MODIFIED"
echo ""

if [ "$ROOT" != "$ROOT_MODIFIED" ]; then
    echo "✅ SUCCESS: Root hash changed! Merkle tree detected the modification."
else
    echo "❌ ERROR: Root hash should have changed."
fi
echo ""
echo "Press Enter to continue..."
read -r
echo ""

echo ""
echo "4. Why This Matters:"
echo "• You can verify ALL files by checking just the root hash"
echo "• No need to download/compare every file individually"
echo "• Git uses this for commit verification"
echo "• Nix uses this for build reproducibility"
echo "• Eka uses this for package integrity"
echo ""

echo "5. Real-world example - Git commit:"
echo "Run: git log --oneline -1"
echo "This shows Git's Merkle tree root (commit hash)"
echo ""

# Show a real git example if we're in a git repo
if git rev-parse --git-dir > /dev/null 2>&1; then
    echo "Current Git commit (Merkle root):"
    git rev-parse HEAD
    echo ""
    echo "This hash represents the entire state of ALL files in the repository!"
else
    echo "Not in a Git repository, but imagine this hash represents your entire codebase."
fi