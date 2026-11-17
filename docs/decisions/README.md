# Design decisions

Decision records explain choices that affect several SessionDB components. Requirements accepted in the product conversation are distinct from implementation proposals awaiting review.

| Record | Status | Decision |
| --- | --- | --- |
| [0001 Product direction](0001-product-and-rust.md) | Accepted; identity assumptions superseded by 0013 | Session database and documentation-first development; implementation language recorded in 0001 |
| [0002 Storage and formats](0002-storage-and-formats.md) | Accepted direction | Native originals, PostgreSQL application model, existing S3 storage, derived exports |
| [0003 Interfaces](0003-interfaces.md) | Accepted direction | First-class CLI, HTTP API/OpenAPI, reusable client SDK |
| [0004 Rust architecture](0004-rust-architecture.md) | Accepted; identity-library assumptions superseded by 0013 | Five-package workspace, async modular server, durable background jobs |
| [0005 Search](0005-search.md) | Proposed; identity scope superseded by 0013 | Keyword baseline and optional profiled embeddings with exact initial vector search |
| [0006 Team access](0006-team-access.md) | Superseded by 0008 and 0013 | Historical project-bound access proposal; no active identity policy |
| [0007 Capability focused documentation](0007-capability-focused-documentation.md) | Accepted; identity-feature assumptions superseded by 0013 | Lead with storing, syncing, querying, and searching sessions; document shared access as a supporting capability |
| [0008 Collection and project associations](0008-collection-and-project-associations.md) | Accepted direction; access assumptions superseded by 0013 | Source-based collection and optional project labels |
| [0009 Reference storage and dependencies](0009-reference-storage-and-dependencies.md) | Accepted | Tested SeaweedFS/AWS-client pair and compiler/library baseline |
| [0010 Authentication](0010-reference-authentication.md) | Superseded by 0013 | Historical Keycloak code/PKCE profile and API audience |
| [0011 Normalized messages](0011-normalized-message-contract.md) | Accepted | Typed blocks, provider normalization, bounded previews, and native fixtures |
| [0012 First build](0012-first-build-scope.md) | Count/identity operations superseded by 0013 | Historical 23-operation slice; now 18 baseline plus two artifact reads |
| [0013 Instance API key](0013-instance-api-key.md) | Accepted | One shared collection and required instance-wide key; clean identity cutover |

An accepted direction does not mean every example field or default has been approved. Accept a proposed record after its assumptions and relevant open questions are reviewed. Supersede records with a new linked decision rather than erasing the rationale.
