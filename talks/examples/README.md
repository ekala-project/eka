# Presentation Examples

This directory contains interactive examples to complement the "From Nix to Eka" presentation. Each example demonstrates key concepts with hands-on code that students can run and modify.

## Setup

First, enter the Nix shell with all required tools:

```bash
cd talks/examples
nix-shell shell.nix
```

This provides:
- `openssl` for hashing
- `jq` for JSON processing
- `git` for version control demos
- `nix` for derivation examples

## Examples

### 1. Environment Degradation (`01-environment-degradation.sh`)

**When to run:** During the "The Problem" section

**What it shows:**
- How environments accumulate changes over time
- Why "works on my machine" fails
- File conflicts and permission issues
- The mess that traditional package management creates

**Run it:**
```bash
./01-environment-degradation.sh
```

**Key takeaway:** Environments degrade naturally, making reproducibility impossible without strict controls.

### 2. Merkle Tree Demo (`02-merkle-tree-demo.sh`)

**When to run:** During the "Basic Cryptography" section

**What it shows:**
- How cryptographic hashes work
- Building a simple Merkle tree from scratch
- How changing one file affects the entire tree
- Real Git commit hash example

**Run it:**
```bash
./02-merkle-tree-demo.sh
```

**Key takeaway:** Merkle trees enable efficient verification of large datasets with minimal computation.

### 3. Static Build Recipes (`03-static-build-recipes.sh`)

**When to run:** During the "How Nix Solves It" section

**What it shows:**
- Traditional build scripts vs. declarative recipes
- JSON build specifications
- Nix derivation structure and content-addressing
- Real derivation examples from the Nix store

**Run it:**
```bash
./03-static-build-recipes.sh
```

**Key takeaway:** Static, cryptographic build recipes enable perfect reproducibility across time and machines.

### 4. Nixpkgs Coupling Pain (`04-nixpkgs-pain-demo.sh`)

**When to run:** During the "What's Wrong with Nix" section

**What it shows:**
- The pain of nixpkgs version management
- Tight coupling between all packages
- Version conflicts and rebuild costs
- Why nixpkgs makes complex dependency management hard

**Run it:**
```bash
./04-nixpkgs-pain-demo.sh
```

**Key takeaway:** Nix's monolithic approach creates scaling and flexibility problems that Eka/Atoms solve through decoupling and proper abstraction.

### 5. Nix Language Introduction (`05-nix-language-intro.sh`)

**When to run:** During the "Building Derivations" section

**What it shows:**
- What Nix expressions look like
- Key Nix concepts (pure functions, declarative)
- Why Nix expressions are powerful but complex
- Sets up understanding for why Eka abstracts them

**Run it:**
```bash
./05-nix-language-intro.sh
```

**Key takeaway:** Nix expressions enable reproducible builds but are too complex for most users - this is what Eka abstracts away.

### 6. Docker Limitations (`06-docker-limitations-demo.sh`)

**When to run:** During the "Existing Tools Don't Solve This" section (after mentioning Docker)

**What it shows:**
- Why containers don't solve reproducibility
- Base image drift and supply chain attacks
- Dependency conflicts inside containers
- Docker's strengths vs. limitations

**Run it:**
```bash
./05-docker-limitations-demo.sh
```

**Key takeaway:** Docker is great for deployment but doesn't address the core dependency management and reproducibility problems that Eka/Atoms solve.

## Presentation Flow

The examples are numbered to match the slide progression:

1. **Slides 2-3:** Run example 1 (environment degradation)
2. **Slides 4-5:** Run example 2 (Merkle trees)
3. **Slides 6-7:** Run example 3 (build recipes)
4. **Slide 8:** Run example 5 (Docker limitations) after mentioning containers
5. **Slides 9-10:** Run example 4 (nixpkgs coupling pain) + live demo

## Tips for Presenting

- **Pause for questions:** After each example, ask "What would happen if...?"
- **Encourage modification:** Let students tweak the scripts and see results
- **Connect to real world:** Relate examples to their own development experiences
- **Time management:** Each example takes 3-5 minutes to run and explain

## Technical Notes

- All examples are self-contained and safe to run
- Temporary files are cleaned up automatically
- Examples work on any Unix-like system with Nix
- No external network access required
- Scripts include detailed explanations and expected output

## Extending the Examples

To add more examples:

1. Create a new script: `04-your-example.sh`
2. Make it executable: `chmod +x 04-your-example.sh`
3. Add required tools to `shell.nix`
4. Update this README
5. Reference it in the slides

The examples are designed to be living demonstrations that evolve with the presentation!