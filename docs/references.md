# References and conventions

These primary sources cover the formats, interfaces, and components used in SessionDB's design.

Reviewed on 2026-11-15. Native formats and evolving AI conventions require rechecking when adapters or integrations are implemented.

## Rust

- [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html) — shared Cargo metadata, commands, dependencies, and lockfile.
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) — naming, interoperability, documentation, and public client ergonomics.
- [axum](https://docs.rs/axum/latest/axum/) and [Tokio](https://docs.rs/tokio/latest/tokio/) — candidate service/runtime.
- [clap](https://docs.rs/clap/latest/clap/) — conventional command parsing, help, and completions.
- [SQLx](https://docs.rs/sqlx/latest/sqlx/) — candidate PostgreSQL integration.
- [reqwest](https://docs.rs/reqwest/latest/reqwest/) — candidate reusable HTTP client.
- [Apache Arrow object_store](https://docs.rs/object_store/latest/object_store/) — candidate blob abstraction; verify required signing features before selection.

## APIs and operations

- [PostgreSQL overview](https://www.postgresql.org/about/) and [MLflow documentation](https://mlflow.org/docs/latest/) — capability-focused introductions and documentation organized around using the software.
- [OpenAPI 3.1.1](https://spec.openapis.org/oas/v3.1.1.html) — language-neutral HTTP contract baseline.
- [RFC 9110 HTTP Semantics](https://www.rfc-editor.org/rfc/rfc9110.html) — methods, conditional requests, and ranges.
- [RFC 9457 Problem Details](https://www.rfc-editor.org/rfc/rfc9457.html) — structured HTTP errors.
- [OpenID Connect Core](https://openid.net/specs/openid-connect-core-1_0.html) and [RFC 8252 OAuth for Native Apps](https://www.rfc-editor.org/rfc/rfc8252.html) — historical OIDC references for the superseded authentication experiments, not current deployment requirements.
- [S3 GetObject](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetObject.html), [conditional writes](https://docs.aws.amazon.com/AmazonS3/latest/userguide/conditional-writes.html), and [metadata](https://docs.aws.amazon.com/AmazonS3/latest/userguide/UsingMetadata.html) — object access conventions; an ETag is not universally a content digest.
- [psql](https://www.postgresql.org/docs/current/app-psql.html) — a first-class CLI client for a database service.
- [Vector collection roles](https://vector.dev/docs/setup/deployment/roles/) — host collectors and central processing as separate operating roles.
- [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) — commit message format.
- [pgvector](https://github.com/pgvector/pgvector) — vector storage/search and model/index dimensional constraints.

## Sessions and trace interchange

- [Codex rollout and history source](https://github.com/openai/codex/tree/rust-v0.169.0/codex-rs/rollout) and [fork handling](https://github.com/openai/codex/blob/rust-v0.169.0/codex-rs/thread-store/src/local/paginated_fork.rs) — version-specific native implementation evidence.
- [Claude Code session storage](https://code.claude.com/docs/en/sessions) and [directory layout](https://code.claude.com/docs/en/claude-directory) — native transcript and supporting-file locations.
- [Pi session format](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/session-format.md) — native tree, context changes, and message metadata.
- [Hugging Face Agent Traces](https://huggingface.co/docs/hub/agent-traces) and [STS format](https://huggingface.co/docs/hub/session-traces-format) — native viewing and a simple export projection.
- [Harbor ATIF](https://docs.harborframework.com/agents/atif) — trajectory interchange; producing and resuming a trajectory are separate capabilities.
- [OpenTelemetry GenAI conventions](https://github.com/open-telemetry/semantic-conventions-genai) and [OpenInference](https://arize-ai.github.io/openinference/spec/) — execution observability; preserve actual versus missing timing.
- [Agent Trace](https://agent-trace.dev/) — code attribution and conversation links, distinct from execution tracing.
- [Anthropic agent evaluation guidance](https://www.anthropic.com/research/demystifying-evals-for-ai-agents) — recorded interactions and verified environment outcomes are different evidence.

## Adjacent products

[Hugging Face](https://huggingface.co/docs/hub/agent-traces), [Entire Sessions](https://entire.com/features/sessions), [ccusage](https://ccusage.com/guide/all-reports), and [Langfuse](https://langfuse.com/docs/observability/overview) demonstrate related storage, browsing, analytics, and observability workflows. SessionDB focuses on storing sessions, syncing native history automatically, and providing consistent queries and search.
