# Eka Trust Model: Proof-of-Useful-Work Consensus for Decentralized Package Management

## Executive Summary

Eka's trust model introduces **proof-of-useful-work consensus** as a novel mechanism for establishing trust in decentralized package ecosystems. Unlike traditional proof-of-work systems that waste computational resources on meaningless puzzles, proof-of-useful-work derives authority from demonstrated value to the ecosystem through meaningful software contributions.

This document establishes proof-of-useful-work as the central innovation of Eka's trust model, grounded in the current Eka implementation and ROADMAP.md specifications. Trust emerges from network consensus among mirrors that have proven their value through sustained, high-impact contributions to the software ecosystem.

## Architectural Motivation

### The Trust Crisis in Software Distribution

Traditional package management suffers from fundamental trust issues:

- **Centralized registries** create single points of failure and control
- **Institutional authority** relies on human processes prone to compromise
- **No accountability** for package quality or maintainer legitimacy
- **Supply chain attacks** exploit trust assumptions rather than cryptographic weaknesses

Eka addresses these through cryptographic foundations and decentralized consensus, but the critical question remains: _how do we establish trust in a decentralized system without centralized authorities?_

### Eka's Current Implementation Foundation

Eka's existing architecture provides the cryptographic foundation for trust:

**Atom Identity**: Each atom has a cryptographically unique ID derived from its repository's root commit hash and human-readable label, preventing impersonation attacks.

**Git-Native Publishing**: Atoms are published as reproducible Git commits with references under `refs/eka/atoms/<label>/<version>`, ensuring content-addressed integrity.

**Decentralized Resolution**: Dependencies resolve from multiple mirrors simultaneously, with cryptographic verification ensuring integrity regardless of source.

**Repository Identity**: Entropy-injected initialization commits provide temporal provenance and fork disambiguation.

These mechanisms eliminate dependency confusion and provide mathematical authenticity guarantees, but they don't address _trustworthiness_ - the quality, security, and legitimacy of the software itself.

### The Proof-of-Useful-Work Innovation

Proof-of-useful-work consensus represents Eka's core contribution to decentralized trust systems. Instead of computational waste or financial stake, trust authority derives from demonstrated value to the ecosystem:

- **Stakeholder Empowerment**: Trust authority comes from actual developers and project contributors
- **Quality over Quantity**: Contribution metrics prioritize impact (code reach, usage frequency, bug fixes) over simplistic measures
- **Natural Decay**: Reputation decreases with inactivity but never drops below neutral unless malicious behavior occurs
- **Consensus-Based**: Network consensus validates individual contributions, preventing gaming

This creates a self-reinforcing ecosystem where cooperation is the rational choice, malicious behavior is quickly detected and penalized, and the network maintains security through collective intelligence rather than institutional authority.

## Core Trust Mechanisms

### Proof-of-Useful-Work Consensus

The central innovation of Eka's trust model is **proof-of-useful-work consensus**, where trust authority derives from demonstrated value to the ecosystem rather than computational waste or financial stake.

#### Key Properties

**Non-naive Contribution Metrics**: Quality is measured by code impact rather than simplistic metrics:

- **Code Reach**: How many other components reference the contribution
- **Usage Frequency**: How often the contributed code is used
- **Longevity**: Sustained contributions over time
- **Consistency**: Regular, reliable participation

**Natural Reputation Dynamics**:

- Reputation decreases with inactivity but never drops below neutral (0.5)
- Malicious behavior causes permanent reputation loss
- Trust poisoning from non-contributing entities is heavily discounted
- Stakeholder empowerment places authority in hands of actual developers

**Outstanding Research Questions**:

- How to quantify "useful work" without gaming the metrics?
- Should contribution metrics be global (across all projects) or project-specific?
- How to handle contributors who work on private repositories?
- What minimum contribution threshold establishes trust authority?
- How does this interact with automated systems (CI/CD bots, etc.)?

### Trust Assertion Storage

Trust assertions are stored as signed Git tags with structured metadata, building on Eka's existing Git-native publishing infrastructure.

#### Git Tag Structure

```toml
[trust.assertions]
maintainers = [
  {key = "0x...", level = "author", evidence = "commit_history"}
]
contributors = [
  {key = "0x...", level = "verified", evidence = "pull_requests"}
]

[trust.revocations]
compromised_keys = [
  {key = "0x...", reason = "key_leak", timestamp = "2024-01-01T00:00:00Z"}
]

[trust.metadata]
build_integrity = "blake3:..."
last_verified = "2024-01-01T00:00:00Z"
```

Tags are created under `refs/eka/trust/<atom-id>/<version>` following the pattern established in ROADMAP.md for build metadata storage.

### Consensus Algorithm

Network consensus weights individual mirror assertions by reputation scores:

1. **Weighted Voting**: Each mirror's assertion is weighted by its proof-of-useful-work reputation
2. **Temporal Weighting**: Recent assertions carry higher weight with exponential decay
3. **Evidence-Based Scoring**: Assertions backed by verifiable evidence receive bonus weighting
4. **Conflict Resolution**: Multiple trust branches maintained when consensus cannot be reached

### Reputation Dynamics

Reputation evolves according to:

```
R_{t+1} = R_t + α(Consensus_Alignment) - β(Divergence_Penalty) - γ(Temporal_Decay) + δ(Evidence_Quality)
```

Where:

- **Consensus_Alignment**: How well assertions align with network consensus
- **Divergence_Penalty**: Applied when assertions significantly deviate from consensus
- **Temporal_Decay**: Ensures reputation requires active maintenance
- **Evidence_Quality**: Bonus for well-substantiated trust claims

## Game-Theoretic Foundation

### Players and Strategies

The trust model operates as a repeated game among mirrors in the Eka network:

- **Players**: Individual mirrors (Git repositories) maintaining trust assertions
- **Strategies**:
  - _Cooperative_: Provide accurate trust assertions based on proof-of-useful-work
  - _Defective_: Spread false information to manipulate consensus
  - _Free-rider_: Consume trust data without contributing assessments
  - _Isolated_: Maintain independent assessments without network participation

### Payoff Structure

Payoffs consider multiple dimensions weighted by proof-of-useful-work reputation:

1. **Reputation Score (R)**: Determined by contribution quality and consensus alignment
2. **Network Access (A)**: Ability to discover and access atoms from other mirrors
3. **Computational Cost (C)**: Resources spent on trust verification and maintenance
4. **Risk of Compromise (P)**: Probability of accepting compromised atoms

**Cooperative Strategy Payoff**: High R + High A - Moderate C - Low P
**Defective Strategy Payoff**: Temporary High R + High A - Low C + High P (but rapidly decaying)
**Free-rider Strategy Payoff**: Moderate R + High A - Minimal C + Moderate P
**Isolated Strategy Payoff**: Low R + Low A - High C + Variable P

### Equilibrium Analysis

#### Nash Equilibrium: Cooperative Dominance

Proof-of-useful-work consensus creates cooperative dominance:

1. **Network Effects**: Accurate trust information value increases exponentially with network size
2. **Reputation Decay**: Defection leads to permanent reputation loss; rebuilding costs exceed short-term gains
3. **Consensus Pressure**: Network consensus acts as collective punishment for deviation

#### Prisoner's Dilemma Resolution

The model resolves the prisoner's dilemma through:

1. **Iterated Games**: Persistent trust relationships allow reputation accumulation
2. **Tit-for-Tat Strategy**: Mirrors reciprocate behavior; cooperation is rewarded, defection penalized
3. **Forgiveness Mechanism**: Occasional errors forgiven if overall cooperation maintained

## Implementation Details

### Current Eka Foundation

The trust model builds on Eka's existing Git-native infrastructure:

**Atom Publishing**: Atoms published as reproducible Git commits with references under `refs/eka/atoms/<label>/<version>` (from ROADMAP.md and `crates/atom/src/publish/mod.rs`)

**Repository Identity**: Root commits with entropy injection provide temporal provenance (from `adrs/0009-repository-identity-and-discovery.md`)

**Content Addressing**: BLAKE3 hashing ensures integrity (planned in ROADMAP.md for E2E verification)

### Trust Metadata Storage

Following ROADMAP.md specifications, trust assertions are stored as signed Git tags:

#### Tag Structure and Location

Tags created under `refs/eka/trust/<atom-id>/<version>` containing:

```toml
[trust.assertions]
maintainers = [
  {key = "0x...", level = "author", evidence = "commit_history"}
]
contributors = [
  {key = "0x...", level = "verified", evidence = "pull_requests"}
]

[trust.revocations]
compromised_keys = [
  {key = "0x...", reason = "key_leak", timestamp = "2024-01-01T00:00:00Z"}
]

[trust.metadata]
proof_of_work_score = 0.85
last_verified = "2024-01-01T00:00:00Z"
consensus_weight = 0.92
```

#### Integration with Build Metadata

ROADMAP.md specifies signed tags for build artifacts; trust assertions extend this pattern:

- **Source Trust Tags**: Under `refs/eka/trust/<atom-id>/<version>`
- **Build Trust Tags**: Under `refs/eka/meta/<label>/<version>/<blake3-content-sum>` (from ROADMAP.md)

This creates an integrated trust chain from source code to built artifacts.

### Mirror Trust Assessment

Mirrors assess trust based on proof-of-useful-work metrics:

1. **Contribution Analysis**: Evaluate code impact, usage frequency, longevity
2. **Key Verification**: Validate signing keys against contribution history
3. **Consensus Checking**: Cross-reference with other mirror assessments
4. **Temporal Validation**: Ensure assertions remain current and relevant

### Network Consensus Computation

The consensus algorithm operates on weighted trust assertions:

1. **Reputation Weighting**: Each assertion weighted by mirror's proof-of-useful-work score
2. **Temporal Decay**: Recent assertions carry higher weight with exponential decay
3. **Evidence Validation**: Assertions backed by verifiable evidence receive bonus weighting
4. **Conflict Resolution**: Multiple trust branches maintained when consensus diverges

## Attack Analysis and Mitigations

### Sybil Attacks

**Attack**: Single entity creates multiple mirrors to artificially inflate influence on trust consensus.

**Mitigation**: Proof-of-useful-work consensus requires demonstrated value to the ecosystem. Mirrors must demonstrate meaningful contributions through code impact metrics rather than just existence.

**Key Properties**:

- **Contribution-Based Authority**: Trust derives from actual software contributions, not mere participation
- **Cross-Verification**: Network consensus validates individual contribution claims
- **Reputation Gates**: Low-reputation mirrors have minimal influence on consensus
- **Temporal Validation**: Sustained contribution history required for significant reputation

### Trust Poisoning

**Attack**: Malicious mirrors spread false trust information to undermine legitimate atoms.

**Mitigation**: Multi-layered defense combining evidence requirements, consensus validation, and temporal decay:

- **Evidence-Based Assertions**: Trust claims must be backed by verifiable evidence (code reviews, security audits, usage metrics)
- **Consensus Isolation**: Outlier assertions detected and heavily discounted by network consensus
- **Temporal Decay**: Poisoned assertions lose influence over time without reinforcement
- **Key Revocation**: Compromised signing keys can be revoked through the tag-based revocation system

### Eclipse Attacks

**Attack**: Attacker controls majority of mirrors a victim can see, creating false consensus.

**Mitigation**: Decentralized discovery and reputation-based weighting:

- **Diverse Mirror Discovery**: DHT-based discovery ensures access to multiple independent mirror sets
- **Reputation Weighting**: High-reputation mirrors carry more weight, preventing attacker dominance
- **Geographic Distribution**: Network topology prevents single-entity control of victim-visible mirrors
- **Fork Resolution**: Victims can choose alternative trust branches when consensus is suspect

### Free-Riding

**Attack**: Mirrors consume trust data without contributing assessments.

**Mitigation**: Participation requirements enforced through graduated access:

- **Minimum Contribution Thresholds**: Mirrors must demonstrate basic contribution levels for network access
- **Reputation Requirements**: Higher reputation unlocks more network privileges
- **Consensus Participation**: Active contribution to consensus calculations required for full access
- **Natural Incentives**: Contributing mirrors gain reputation benefits and better network access

## Dual-Axis Trust: Source vs. Artifacts

The trust model operates along two integrated but distinct axes, building on ROADMAP.md's E2E integrity requirements.

### Source Trust (Eka Layer)

- **Governs**: Trustworthiness of source code atoms
- **Managed through**: Mirror consensus and proof-of-useful-work validation
- **Focus**: Code quality, maintainer legitimacy, security audits
- **Storage**: Signed Git tags under `refs/eka/trust/<atom-id>/<version>`
- **Foundation**: Builds on Eka's existing atom publishing infrastructure

### Artifact Trust (Eos Layer)

- **Governs**: Trustworthiness of built binaries and artifacts
- **Cryptographically links**: Source trust through build inputs and content hashing
- **Handles**: Non-bit-for-bit reproducible builds via input hash verification
- **Provides**: End-to-end chain from `atom_hash → version_hash → input_hash → content_hash`
- **Storage**: Signed Git tags under `refs/eka/meta/<label>/<version>/<blake3-content-sum>` (per ROADMAP.md)

### Trust Flow Integration

Source trust establishes the "root of trust" for artifacts:

- Only source-trusted entities can authorize artifact-signing keys
- Build processes inherit trust from their source atoms
- Decentralized verification allows anyone to validate artifacts against signed metadata

**Outstanding Research Questions**:

- How to handle multiple valid `input_hash → content_hash` mappings for non-reproducible builds?
- Should artifact trust decay independently of source trust?
- How to verify build environment integrity (compiler versions, etc.)?
- What constitutes sufficient source trust to authorize artifact signing?
- How to handle cross-ecosystem artifact dependencies?

## Research Questions and Future Work

### Proof-of-Useful-Work Refinements

**Metrics Development**:

- How to quantify "useful work" without gaming the system?
- Should contribution metrics be global (across all projects) or project-specific?
- How to handle contributors who work on private repositories?
- What minimum contribution threshold establishes trust authority?
- How does this interact with automated systems (CI/CD bots, etc.)?

**Impact Metrics**:

- Code reach (how many other components reference the contribution)
- Usage frequency and adoption rates
- Bug fix quality and security impact
- Long-term maintenance and support contributions

### Consensus Algorithm Enhancements

**Formal Verification**:

- Mathematical proofs of cooperative strategy dominance
- Byzantine fault tolerance guarantees
- Scalability bounds for network size vs. consensus reliability

**Advanced Features**:

- Automated fork resolution mechanisms
- Machine learning for adaptive trust thresholds
- Cross-ecosystem trust assertion portability

### Attack Vector Analysis

**Quantum Resistance**: Trust model resilience against quantum computing advances
**State-level Attacks**: Defense against nation-state actors with significant resources
**Economic Attacks**: Protection against bribery or coercion of trusted developers
**Social Engineering**: Mitigation of developer impersonation and social trust exploitation

### Adaptive Trust Thresholds

The system could implement machine learning to dynamically adjust trust requirements based on:

- Atom popularity and usage patterns
- Historical compromise rates
- Network health metrics
- Ecosystem-specific risk profiles

### Cross-Ecosystem Trust

Extend trust assertions to cover relationships between different package ecosystems, enabling secure cross-ecosystem dependencies while maintaining decentralized authority.

This trust model creates a self-reinforcing ecosystem where cooperation is the rational choice, malicious behavior is quickly detected and penalized, and the network maintains security and reliability through collective intelligence rather than centralized authority. Proof-of-useful-work consensus represents a novel contribution to decentralized trust systems, establishing trust authority through demonstrated value rather than computational waste or financial stake.

The model is grounded in Eka's current implementation while providing a foundation for the planned Eos distributed build service, creating an integrated trust framework from source code to final artifacts.
