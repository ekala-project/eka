#!/usr/bin/env bash
# Example 5: Docker Limitations Demo
# Shows why containers don't solve reproducibility problems

set -e

echo "=== Example 5: Docker Limitations Demo ==="
echo ""
echo "This demonstrates why Docker containers, while useful, don't solve"
echo "the fundamental reproducibility and dependency management problems."
echo ""

# Check if Docker is available
if ! command -v docker >/dev/null 2>&1; then
    echo "❌ Docker not found. This demo requires Docker to be installed."
    echo "On most systems: curl -fsSL https://get.docker.com | sh"
    echo ""
    echo "We'll simulate the concepts instead..."
    echo ""
else
    echo "✅ Docker found! Let's explore the limitations."
    echo ""
fi

echo "1. Docker's Promise vs Reality"
echo "Press Enter to continue..."
read -r
echo ""

echo "Docker promises:"
echo "• 'Build once, run anywhere'"
echo "• 'No more 'works on my machine''"
echo "• Complete environment isolation"
echo ""

echo "But in practice:"
echo "• Base images change over time"
echo "• Dependencies inside containers can still conflict"
echo "• No cryptographic verification of contents"
echo "• Supply chain attacks still possible"
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "2. Base Image Drift Problem"
echo "Press Enter to continue..."
read -r
echo ""

cat << 'EOF'
# A simple Dockerfile - looks declarative, right?
FROM ubuntu:latest  # ❌ This line is the problem!

RUN apt-get update && apt-get install -y \
    python3 \
    python3-pip \
    curl

CMD ["python3", "--version"]
EOF

echo ""
echo "This Dockerfile looks perfectly declarative and reproducible."
echo "But look at line 2: FROM ubuntu:latest"
echo ""
echo "The 'latest' tag is NOT a fixed version - it changes over time!"
echo "Docker Hub automatically updates ubuntu:latest when new versions are released."
echo ""

echo ""
echo "The 'ubuntu:latest' tag changes over time!"
echo "Same Dockerfile builds different images on different days."
echo ""

echo "Example: If you build this Dockerfile today vs. next month:"
echo "• Today: Ubuntu 22.04.3 + Python 3.10.12"
echo "• Next month: Ubuntu 22.04.4 + Python 3.10.13 (or newer)"
echo "• Different package versions, different security patches"
echo "• Your app might break unexpectedly!"
echo ""
echo "Press Enter to continue..."
read -r
echo ""

if command -v docker >/dev/null 2>&1; then
    echo "Let's see what 'ubuntu:latest' resolves to today:"
    docker pull ubuntu:latest >/dev/null 2>&1 || true
    docker inspect ubuntu:latest --format='{{.Id}}' 2>/dev/null | head -c 20 || echo "Could not inspect"
    echo " (this will be different tomorrow!)"
    echo ""
fi

echo "Press Enter to continue..."
read -r
echo ""

echo "3. Dependency Conflicts Inside Containers"
echo "Press Enter to continue..."
read -r
echo ""

cat << 'EOF'
# Even inside Docker, you still have dependency management problems!

FROM python:3.9

# Install some packages
RUN pip install requests==2.25.0
RUN pip install urllib3==1.26.0

# What if another part of your app needs:
# requests==2.28.0 (newer)
# urllib3==2.0.0 (incompatible!)

# Docker doesn't solve this - you still need package management!
EOF

echo ""
echo "Docker bundles the environment but doesn't manage dependencies within it."
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "4. Supply Chain Attacks"
echo "Press Enter to continue..."
read -r
echo ""

echo "Docker images can be compromised:"
echo "• Malicious base images on Docker Hub"
echo "• Compromised package installs inside containers"
echo "• No cryptographic verification of image contents"
echo "• Images can be modified after creation"
echo ""

echo "Real examples:"
echo "• Codecov breach (2021): Compromised Docker images via malicious code injection"
echo "• SolarWinds-style attacks: Supply chain compromises affecting container registries"
echo "• Typosquatting: Malicious 'ubuntuu' images on Docker Hub mimicking official ones"
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "5. Docker's Strengths (What It Does Well)"
echo "Press Enter to continue..."
read -r
echo ""

echo "✅ Deployment consistency"
echo "✅ Runtime isolation"
echo "✅ Simplified distribution"
echo "✅ Resource management"
echo ""

echo "❌ Build reproducibility"
echo "❌ Dependency resolution"
echo "❌ Supply chain security"
echo "❌ Version management"
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "6. Why Nix + Docker Could Be Better"
echo "Press Enter to continue..."
read -r
echo ""

cat << 'EOF'
# Nix builds reproducible artifacts, Docker deploys them

# 1. Use Nix to build reproducible packages
# 2. Create minimal Docker images from Nix outputs
# 3. Cryptographic verification from source to deployment

# But even this has limitations:
# - Still need to manage complex Nix expressions manually
# - Docker layers can still have reproducibility issues
# - Complex workflow requiring expertise in both tools
EOF

echo ""
echo "This is where Eka/Atoms could shine - they aim to provide the package management"
echo "that Docker lacks, with the security guarantees of Nix, while hiding the"
echo "complexity behind a clean, user-friendly interface."
echo ""
echo "Eka still uses Nix internally (like how Docker uses the kernel internally),"
echo "but makes Nix expressions an implementation detail that users don't see."
echo "The goal is a complete package management solution,"
echo "unlike Docker + Nix which remains two separate tools to orchestrate."
echo ""

echo "Press Enter to continue..."
read -r
echo ""

echo "7. The Real Solution: Content-Addressed Everything"
echo "Press Enter to continue..."
read -r
echo ""

echo "What we really need:"
echo "• Cryptographically verifiable dependencies"
echo "• Content-addressed storage (like Nix)"
echo "• Decentralized package distribution"
echo "• Proper version resolution and conflict management"
echo ""

echo "This is exactly what Eka/Atoms provide!"
echo ""

echo "=== End of Demo ==="
echo ""
echo "Docker is great for deployment, but doesn't solve the core"
echo "reproducibility and dependency management problems that"
echo "Eka/Atoms address."