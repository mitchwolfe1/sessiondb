# SessionDB contributor instructions

## Product direction

SessionDB is a Rust database for coding-agent sessions. Lead with storing, automatically syncing, accessing, querying, and searching Codex, Claude Code, and Pi sessions through conventional CLI/API/SDK interfaces, with embedding extensibility.

Lead with capabilities and usage, as database and developer-tool documentation does. Shared deployment, users, teams, and permissions are supported features; they are not the product's identity or an exclusive audience. Put administration and policy details in the relevant reference pages. Explain sessions and useful workflows before account structures or internal storage bookkeeping.

Use established Rust, HTTP, CLI, PostgreSQL, and S3 conventions. SessionDB exposes session resources through its API and uses an existing object storage implementation for raw files. Do not implement a new S3 server or invent a SQL language for the CLI.

## Current phase

This repository is documentation first. Until the user explicitly requests implementation, change documentation and contract examples only. Do not add application code, Cargo manifests, migrations, executable deployment files, or generated SDKs. Documentation and example commands must not claim that software already runs.

Read `docs/README.md`, the relevant feature documents, and applicable decision records before editing. Product decisions and implementation proposals have different statuses. Keep the decision register, API descriptions, CLI examples, and acceptance criteria consistent.

Do not treat a list of open questions as permission to stop documentation work. Make concrete proposals where useful and record unresolved choices with their consequences and the milestone they block.

## Changes and validation

- Use Conventional Commits: `docs:`, `feat:`, `fix:`, `refactor:`, `test:`, `build:`, `ci:`, `perf:`, `chore:`, or `revert:` as appropriate.
- Keep commits focused and describe the final change. Do not include real session data, credentials, private source contents, or machine-specific state.
- Use synthetic examples. Preserve native source bytes in the design and distinguish recorded user roles from human authorship.
- Check relative links, document consistency, example syntax, and whitespace. Validate machine-readable contracts when introduced.
- For future Rust work, use the workspace conventions in `docs/architecture.md` and `CONTRIBUTING.md`; select and lock dependency versions at implementation time.

## Boundaries

SessionDB owns collection, storage, session access, and retrieval. Applications such as hype-history own their additional briefing, research, and reviewed-memory workflows. Existing application policy must not silently become SessionDB policy.

Organization and project authorization applies consistently to raw content, parsed content, changes, exports, keyword search, and vector retrieval.
