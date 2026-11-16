# Product direction and Rust

Status: accepted. Date: 2026-11-15.

## Decision

Build SessionDB as a Rust session database and automatic sync product for engineers, teams, and organizations using Codex, Claude Code, and Pi. Organizations are a primary audience. A personal installation uses the same model with one user.

Develop specifications and documentation before application code. Use Conventional Commits for repository changes. The repository is private under the maintainer's personal GitHub account.

## Consequences

The server, CLI, collection agent, and first native SDK are Rust components. An existing Python prototype can inform behavior and tests without making Python part of the server runtime. Other-language API clients are integrations, not a change to the implementation language.

Team membership, shared projects, authorization, useful APIs, and embedding extensibility shape the product. Storage bookkeeping remains an internal engineering concern. Implementation starts only after the relevant design is reviewed and explicitly requested.
