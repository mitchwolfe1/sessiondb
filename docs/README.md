# SessionDB documentation

SessionDB stores, syncs, queries, and searches coding-agent sessions. These documents cover its planned CLI, API, data model, Rust implementation, and server configuration.

## Find a task

| Task | Start here |
| --- | --- |
| Understand SessionDB | [Product specification](product.md) |
| Collect and browse sessions | [CLI](cli.md), [automatic sync](sync.md) |
| Query sessions from an application | [API](api.md), [OpenAPI](openapi.yaml), [data model](data-model.md) |
| Configure search | [Search and embeddings](search-and-embeddings.md) |
| Add a harness or integration | [Adapters](harness-adapters.md), [integrations](integrations.md) |
| Run a server or configure shared access | [Operations](operations.md), [access control](access-control.md) |
| Contribute to the project | [Architecture](architecture.md), [delivery plan](roadmap.md), [contribution guide](../CONTRIBUTING.md) |

## Design status

The repository is in the documentation phase. Commands, endpoints, and configuration examples describe planned behavior. The implementation will use Rust, support Codex/Claude/Pi, preserve native files, and provide automatic sync and extensible search.

The [decision register](decisions/README.md) distinguishes accepted direction from proposed technical choices. [Open questions](open-questions.md) identify choices needed for upcoming milestones.

The [product specification](product.md) describes scope. Feature pages describe behavior; [API](api.md) and [OpenAPI](openapi.yaml) describe the HTTP interface and wire schemas. See [CONTRIBUTING.md](../CONTRIBUTING.md) for updating them together.

## Complete document map

- [Product specification](product.md)
- [CLI and Rust SDK](cli.md)
- [Automatic sync](sync.md)
- [Harness adapters](harness-adapters.md)
- [Data model](data-model.md)
- [HTTP API](api.md)
- [OpenAPI schema](openapi.yaml)
- [Search and embeddings](search-and-embeddings.md)
- [Integrations and formats](integrations.md)
- [Architecture and Rust workspace](architecture.md)
- [Operations](operations.md)
- [Access control](access-control.md)
- [Delivery and acceptance](roadmap.md)
- [Open questions](open-questions.md)
- [Design decisions](decisions/README.md)
- [References](references.md)

## Contract examples

The [examples directory](examples/README.md) contains illustrative configuration and API documents. They are documentation fixtures, not executable deployment files. Use the same synthetic organization, projects, and IDs throughout the examples so the complete product flow can be reviewed.
