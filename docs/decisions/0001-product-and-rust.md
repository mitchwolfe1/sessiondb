# Product direction and Rust

Status: accepted. Date: 2026-11-15.

Product framing is clarified by [0007 Capability focused documentation](0007-capability-focused-documentation.md).

## Decision

Build SessionDB as a Rust database for coding-agent sessions, with automatic sync from Codex, Claude Code, and Pi. It supports direct CLI use and applications built on its APIs, including shared deployments with users and teams.

Develop specifications and documentation before application code. Use Conventional Commits for repository changes. The repository is private under the maintainer's personal GitHub account.

## Consequences

The server, CLI, collection agent, and first native SDK are Rust components. An existing Python prototype can inform behavior and tests without making Python part of the server runtime. Other-language API clients are integrations, not a change to the implementation language.

Storage, sync, session queries, search, useful APIs, and embedding extensibility define the core product. Shared access is a supported capability; its technical model is documented separately. Implementation starts after the relevant design is reviewed and explicitly requested.
