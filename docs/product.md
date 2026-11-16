# Product specification

SessionDB gives engineering organizations one place to collect, manage, and search coding-agent sessions across engineers, teams, machines, and harnesses. Engineers contribute through an automatic sync client. Colleagues and applications access shared work through a CLI, HTTP API, and SDK.

Status: agreed product direction with proposed v1 requirements. Implementation requires the review gates in the [delivery plan](roadmap.md).

## Customers and users

The primary customer is an engineering organization managing sessions across people and projects. Engineers use it to find previous work, team leads explore project history, and platform or developer-experience teams operate the service and connect internal tools.

Personal use is a single-user organization using the same product. Tool builders are another important audience: memory systems, session browsers, analytics, and evaluation pipelines can use a consistent session interface instead of collecting and interpreting each harness independently. These are target-user hypotheses, not a list of validated commercial customers.

## Main workflows

1. An operator deploys SessionDB, configures identity and storage, and establishes the first organization owner.
2. An owner adds engineers to the organization, creates teams and projects, and grants project access.
3. An engineer signs in, connects local workspaces to projects, and enables automatic collection.
4. Codex, Claude Code, and Pi write sessions normally. SessionDB discovers both new sessions and updates to existing sessions.
5. An authorized colleague lists or searches the project and reads relevant conversations and tool activity.
6. An application uses the same API to supply history to a briefing, browser, or research workflow.
7. An administrator optionally configures an embedding profile and monitors background indexing. Keyword search remains available independently.

The first meaningful demonstration is several engineers, using different harnesses, contributing to one shared project and successfully finding each other's relevant work.

## V1 requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| P01 | Organizations, users, teams, projects, and project grants | A member can access one shared project while being denied another across every read surface. |
| P02 | Native Codex, Claude Code, and Pi collection | Each harness imports real supported record shapes, including tool calls/results and later appends. |
| P03 | Automatic sync | Initial backfill and watch mode work; the client catches up after a restart or network outage without duplicate sessions. |
| P04 | Session access | List, filter, inspect messages and tool activity, edit title/tags, and export a session. |
| P05 | Shared keyword search | Search sessions and messages using project, harness, engineer, and date filters. |
| P06 | Optional embeddings | A configurable provider supports semantic/hybrid search and exposes coverage and job state. |
| P07 | Conventional interfaces | Versioned REST, documented errors/pagination, OpenAPI before route implementation, CLI scripting, and a Rust SDK. |
| P08 | Original file preservation | Retrieve the native files associated with an imported session even if parsing fails. |
| P09 | Operable deployment | A documented single-server setup, authenticated access, migrations, health, logs, backup, and restore. |
| P10 | Extensible adapters | Another harness can use the same adapter boundary without changing storage or CLI commands. |
| P11 | Administrative control | Owners manage membership and grants, source credentials can be revoked, and project maintainers can delete sessions. |
| P12 | Application integration | hype-history can consume sessions and search through the public API without reading SessionDB's database. |

## Product principles

**Sessions are the public abstraction.** Storage hashes, cursor bookkeeping, and object paths are implementation details. Users should primarily work with organizations, projects, engineers, sessions, messages, and search.

**Use familiar contracts.** Use Rust/Cargo conventions, standard HTTP semantics, PostgreSQL, an existing S3-compatible service, structured configuration, and operating-system service managers. Reuse a complete protocol rather than giving a custom protocol a familiar name.

**Collect through explicit project mappings.** Discover harness installations automatically, but publish only sessions matching configured workspaces or explicitly selected sessions. Team collection must not silently sweep an engineer's unrelated personal conversations into a company project.

**Make state understandable.** The client reports pending collection, accepted imports, and failures. The server distinguishes uploaded, parsed, keyword-indexed, and embedded content. Search explains which retrieval mode and coverage it used.

**Keep enrichment optional.** Storage, collection, reading, and keyword search work without an embedding service or model account.

## V1 boundaries

V1 is self-hosted, with Linux servers and Linux/macOS clients. The schema supports organizations, while the deployment experience initially bootstraps one organization per installation. Multiple teams and projects are core.

A browser UI, hosted billing, organization self-signup, SCIM provisioning, a marketplace of plugins, native harness resume, write-back into local transcript files, deterministic execution replay, model training, and employee performance scoring are outside v1. Aggregate analytics and evaluation exports may be built on the API without becoming prerequisites for the session service.

SessionDB stores recorded interactions. A `user` message can be tool output or injected context depending on the harness; it does not automatically identify a human author. An assistant's completion message is not independent verification of an external outcome.

## Evaluation of the product

Track onboarding completion, collection freshness, recoverable failures, time to the first useful shared search result, and whether a second application can use the public API. Numerical latency and capacity targets require a benchmark on the selected Rust/storage implementation; the documentation does not promise unmeasured performance.

The [acceptance scenarios](roadmap.md#acceptance-scenarios) define the initial release gate. Customer interviews and pilots should test whether shared session access changes an actual workflow before expanding enterprise scope.
