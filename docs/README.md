# SessionDB documentation

SessionDB collects and serves coding-agent sessions for organizations. These documents define the product and proposed Rust implementation before application development begins.

## Reading paths

| Reader | Start here |
| --- | --- |
| Product reviewer | [Product](product.md), [delivery plan](roadmap.md), [open questions](open-questions.md) |
| Engineer or team lead | [CLI](cli.md), [access control](access-control.md), [data model](data-model.md) |
| Server or client implementer | [Architecture](architecture.md), [API](api.md), [sync](sync.md), [adapters](harness-adapters.md) |
| Search implementer | [Search and embeddings](search-and-embeddings.md), [integrations](integrations.md) |
| Operator | [Operations](operations.md), [access control](access-control.md) |

## Status and authority

- **Agreed direction** records the product constraints established for this project: Rust, documentation first, organizations and teams as primary users, Codex/Claude/Pi support, automatic sync, conventional interfaces, native originals, and embedding extensibility.
- **Proposed design** specifies concrete behavior for review. Most route, schema, dependency, and operational details are in this category until their decision record is accepted.
- **Open question** names a bounded choice and the implementation milestone it blocks.

All command examples and endpoints are proposed. No implementation exists. A proposed contract is precise enough to review; it is not a claim that a feature has shipped.

The [product specification](product.md) owns scope and requirements. Feature documents own behavior. The [API contract](api.md) and [OpenAPI draft](openapi.yaml) own HTTP behavior and wire schemas; the [CLI contract](cli.md) maps commands to them. [Decision records](decisions/README.md) explain cross-cutting choices. These contracts must agree before code is merged.

If documents conflict, correct them together. A more detailed example does not silently override an agreed product requirement.

## Complete document map

- [Product specification](product.md)
- [Architecture and Rust workspace](architecture.md)
- [Data model](data-model.md)
- [Access control](access-control.md)
- [Automatic sync](sync.md)
- [HTTP API](api.md)
- [OpenAPI schema](openapi.yaml)
- [CLI and Rust SDK](cli.md)
- [Harness adapters](harness-adapters.md)
- [Search and embeddings](search-and-embeddings.md)
- [Operations](operations.md)
- [Integrations and formats](integrations.md)
- [Delivery and acceptance](roadmap.md)
- [Open questions](open-questions.md)
- [Design decisions](decisions/README.md)
- [References](references.md)

## Contract examples

The [examples directory](examples/README.md) contains illustrative configuration and API documents. They are documentation fixtures, not executable deployment files. Use the same synthetic organization, projects, and IDs throughout the examples so the complete product flow can be reviewed.
