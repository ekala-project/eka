# Contributing to Eka

First off, thank you for considering contributing to Eka! It's people like you that make Eka such a great tool.

## Project Ethos and Code of Conduct

This project operates under a specific ethos that values open collaboration, free speech, and technical merit. We have a detailed document, [`ETHIC`](./ETHIC), which we encourage you to read. The core principles are:

- **No Out-of-Band Political Maneuvering**: All discussions and decisions happen in the open.
- **A Fundamental Right to Contribute**: Anyone can contribute, and all contributions will be considered.
- **Focus on Open Collaboration and Free Speech**: We encourage open and honest discussion.

While everyone has the right to contribute, this does not guarantee that your contribution will be merged. All contributions are evaluated based on their technical merit and the contributor's ability to collaborate effectively with the team.

## Getting Started

### Development Environment

The project uses a Nix-based development environment to ensure consistency. The `nix-shell` command will set up all the necessary tools, including the correct Rust toolchain (as specified in `rust-toolchain.toml`) and a fully configured `rust-analyzer`.

For a smoother workflow, we recommend using `direnv`, which can automatically load the Nix shell whenever you enter the project directory.

### Project Overview

For a general overview of the project, please see the [`README.md`](./README.md). To understand our future plans and where the project is headed, check out the [`ROADMAP.md`](./ROADMAP.md).

## How to Contribute

### Communication

Join our [Discord server](https://discord.gg/DgC9Snxmg7) for informal chat, to ask questions, and to collaborate with other contributors.

### Style Guide

All contributions must adhere to our [`STYLE_GUIDE.md`](./STYLE_GUIDE.md). Please read it before you start coding.

### Changesets

We use changesets to manage versioning and changelogs. A changeset is a file that describes the changes you've made. We'll provide more detailed instructions on this soon, but for now, be prepared to include a changeset with your pull request.

### Proposing Major Changes

For substantial changes to the codebase or ecosystem, we use a more formal proposal process to ensure that all stakeholders have an opportunity to provide feedback.

#### Architecture Decision Records (ADRs)

For major technical changes that affect a single repository (e.g., a significant refactoring or the introduction of a new core feature), we use Architecture Decision Records (ADRs). The process is as follows:

1.  **Copy the Template**: Copy the `0000-template.md` from the `/adrs` directory to a new file, such as `/adrs/000X-my-feature.md`.
2.  **Write the Proposal**: Fill out the template with a detailed explanation of the proposed change, including the context, decision, and consequences.
3.  **Submit a Pull Request**: Open a pull request with the new ADR. This will serve as the forum for discussion and review.

#### Ekala Enhancement Proposals (EEPs)

For higher-level proposals that affect the entire Ekala ecosystem or require coordination across multiple projects, we use the Ekala Enhancement Proposal (EEP) process. These proposals are managed in a dedicated repository.

- **EEP Repository**: [https://github.com/ekala-project/eeps](https://github.com/ekala-project/eeps)

Please follow the contribution guidelines in that repository to submit an EEP.

## Other Resources

For more information about other related projects, visit the [ekala-project GitHub organization page](https://github.com/ekala-project).
