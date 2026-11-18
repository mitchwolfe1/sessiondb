# Contributing

SessionDB is private and follows documentation-first development. The initial implementation follows the accepted specification; [implementation status](docs/implementation.md) separates working capabilities from the remaining plan.

## Documentation changes

Read the [documentation guide](docs/README.md). Update the feature specification, related API and CLI examples, and acceptance criteria together. Record an architectural choice in [the decision register](docs/decisions/README.md) when it affects several components.

Lead with what SessionDB does and how to use it. Put access settings and deployment details in their reference pages; keep introductory documentation focused on sessions, sync, queries, search, and integrations.

State whether a choice is an agreed requirement, a proposed implementation, or an unresolved question. Resolve contradictions rather than adding another competing example. Keep examples synthetic and runnable-looking examples explicitly identified as proposed while implementation is absent.

Follow [ADR 0014](docs/decisions/0014-sessions-and-synchronous-files.md) for the sessions-first catalog and synchronous per-file API. Update shared types, multipart/OpenAPI contract, examples, config, CLI `pending_files`, and recovery together. No batch/job compatibility paths or deployment-upgrade migrations belong in this undeployed cutover. Preserve historical checks as history, never as proof of new behavior.

Follow [ADR 0015](docs/decisions/0015-strict-native-records-and-simple-reads.md) for strict whole-file rejection before storage and simple reads without parser diagnostics or revisions. Keep raw rejection fixtures and accepted precision coverage; upload `expected_version` remains the independent write-CAS protocol. Record integrated verification only after it runs.

Before committing, check that relative links resolve, fenced examples are balanced, JSON/TOML examples parse where applicable, and `git diff --check` passes. An OpenAPI contract must be validated and compared against the resource inventory before a route is implemented.

## Commit messages

Use Conventional Commits:

```text
docs: describe session queries
feat(sync): collect updates to existing sessions
fix(api): require the instance key for exports
refactor(adapters): share content block conversion
test(sync): cover reconnect after server outage
```

Use `!` or a `BREAKING CHANGE:` footer for a breaking public contract change. The scope is optional. Commits should contain one coherent change, and their descriptions should explain the resulting behavior.

## Building and testing

The workspace uses Rust 2024 with a tested Rust 1.94.1 baseline. Keep `rust-toolchain.toml` and `Cargo.lock` committed. See [development](docs/development.md) for native build dependencies, local services, and integration checks.

Required checks include:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```

Add integration tests for meaningful behavior: sync and recovery, provider normalization, instance-key rejection and rotation, equal access for all correct-key clients, optional project labels, and search coverage as those features ship. The Docker integration suite uses disposable PostgreSQL and S3-compatible services with synthetic transcripts. Keep embedding tests deterministic and avoid paid model calls.

Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) for the public client crate. Keep network DTOs, internal database models, and native harness payloads distinct. Avoid publishing internal server types through the SDK.

## Reviews and releases

Implementation pull requests identify the requirement and acceptance scenario they satisfy. Keep the checked-in API contract and Rust client synchronized, and generate a contract diff for public API changes. Use Semantic Versioning for released crates and binaries; the `/v1` HTTP namespace has its own compatibility policy.

A release should include documentation, migrations, binary checksums, supported-platform artifacts, and upgrade/rollback instructions. The exact publishing workflow and license remain decisions for the first release, not claims about available artifacts today.
