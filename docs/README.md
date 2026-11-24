# SessionDB documentation

SessionDB stores and syncs coding-agent sessions. These documents cover the CLI, API, data model, architecture, server configuration, and search features.

Start with the [main objects and eleven catalog tables](data-model.md): sessions, messages, events, artifacts, optional projects, source provenance, original generations, and immutable objects. [ADR 0014](decisions/0014-sessions-and-synchronous-files.md) defines synchronous per-file upload and independent file receipts, replacing the historical import/job protocol.

## Find a task

| Task | Start here |
| --- | --- |
| Understand SessionDB | [Product specification](product.md) |
| Run the current implementation | [Build and run](development.md), [implementation status](implementation.md) |
| Start implementation | [First-build scope](first-build.md), [reference validation](validation/reference-stack.md) |
| Collect and browse sessions | [CLI](cli.md), [automatic sync](sync.md) |
| Organize sessions by project | [Project associations](project-associations.md) |
| Query sessions from an application | [API](api.md), [OpenAPI](openapi.yaml), [data model](data-model.md) |
| Understand message blocks | [Normalized content](normalized-content.md), [native fixtures](../fixtures/README.md) |
| Configure search | [Search and embeddings](search-and-embeddings.md) |
| Add a harness or integration | [Adapters](harness-adapters.md), [integrations](integrations.md) |
| Run a server or configure shared access | [Operations](operations.md), [access control](access-control.md) |
| Contribute to the project | [Architecture](architecture.md), [delivery plan](roadmap.md), [contribution guide](../CONTRIBUTING.md) |

## Design status

The initial implementation supports authenticated sync, original storage, session reads, keyword and optional hybrid search, branch-aware context and native export across Codex/Claude/Pi. [Implementation status](implementation.md) identifies the working subset; the remaining target-v1 specification describes later milestones. CLI help and `/v1/info` report available commands/capabilities.

The [decision register](decisions/README.md) distinguishes accepted direction from proposed technical choices. [Open questions](open-questions.md) identify choices needed for upcoming milestones.

The [product specification](product.md) describes scope. Feature pages describe behavior; [API](api.md) and [OpenAPI](openapi.yaml) describe the HTTP interface and wire schemas. See [CONTRIBUTING.md](../CONTRIBUTING.md) for updating them together.

## Complete document map

- [Product specification](product.md)
- [CLI and client SDK](cli.md)
- [Automatic sync](sync.md)
- [Project associations](project-associations.md)
- [Harness adapters](harness-adapters.md)
- [Data model](data-model.md)
- [Normalized content](normalized-content.md)
- [Native fixture pack](../fixtures/README.md)
- [HTTP API](api.md)
- [OpenAPI schema](openapi.yaml)
- [Search and embeddings](search-and-embeddings.md)
- [Integrations and formats](integrations.md)
- [Architecture](architecture.md)
- [Build and run](development.md)
- [Implementation status](implementation.md)
- [First-build scope](first-build.md)
- [Reference stack validation](validation/reference-stack.md)
- [Operations](operations.md)
- [Access control](access-control.md)
- [Authentication](authentication.md)
- [Delivery and acceptance](roadmap.md)
- [Open questions](open-questions.md)
- [Design decisions](decisions/README.md)
- [References](references.md)

## Contract examples

The [examples directory](examples/README.md) contains illustrative configuration and API documents. They are documentation fixtures, not executable deployment files. Use the same synthetic projects and resource IDs throughout the examples so the complete product flow can be reviewed.
