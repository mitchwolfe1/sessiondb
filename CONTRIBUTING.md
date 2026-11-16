# Contributing

SessionDB is currently a private, documentation-first Rust project. The first deliverable is a coherent specification that can guide implementation and review.

## Documentation changes

Read the [documentation guide](docs/README.md). Update the feature specification, related API and CLI examples, and acceptance criteria together. Record an architectural choice in [the decision register](docs/decisions/README.md) when it affects several components.

State whether a choice is an agreed requirement, a proposed implementation, or an unresolved question. Resolve contradictions rather than adding another competing example. Keep examples synthetic and runnable-looking examples explicitly identified as proposed while implementation is absent.

Before committing, check that relative links resolve, fenced examples are balanced, JSON/TOML examples parse where applicable, and `git diff --check` passes. An OpenAPI contract must be validated and compared against the resource inventory before a route is implemented.

## Commit messages

Use Conventional Commits:

```text
docs: define team session access
feat(sync): collect updates to existing sessions
fix(api): apply project access to exports
refactor(adapters): share content block conversion
test(sync): cover reconnect after server outage
```

Use `!` or a `BREAKING CHANGE:` footer for a breaking public contract change. The scope is optional. Commits should contain one coherent change, and their descriptions should explain the resulting behavior.

## Future Rust implementation

The proposed workspace uses the Rust 2024 edition. Choose a tested minimum supported Rust version and commit `rust-toolchain.toml` and `Cargo.lock` when implementation begins. Do not infer a minimum version from one developer's installed toolchain.

Future checks include:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```

These commands do not run against the current documentation-only repository. Add integration tests for meaningful behavior: sync and recovery, provider normalization, organization isolation, project access, and search coverage. Use a real disposable PostgreSQL instance and an S3-compatible test service for storage integration tests; use synthetic transcripts and a deterministic embedding fixture without paid model calls.

Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) for the public client crate. Keep network DTOs, internal database models, and native harness payloads distinct. Avoid publishing internal server types through the SDK.

## Reviews and releases

Implementation pull requests identify the requirement and acceptance scenario they satisfy. Keep the checked-in API contract and Rust client synchronized, and generate a contract diff for public API changes. Use Semantic Versioning for released crates and binaries; the `/v1` HTTP namespace has its own compatibility policy.

A release should include documentation, migrations, binary checksums, supported-platform artifacts, and upgrade/rollback instructions. The exact publishing workflow and license remain decisions for the first release, not claims about available artifacts today.
