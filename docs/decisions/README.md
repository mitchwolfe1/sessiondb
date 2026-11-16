# Design decisions

Decision records explain choices that affect several SessionDB components. Requirements accepted in the product conversation are distinct from implementation proposals awaiting review.

| Record | Status | Decision |
| --- | --- | --- |
| [0001 Product direction](0001-product-and-rust.md) | Accepted | Session database and documentation-first development; implementation language recorded in 0001 |
| [0002 Storage and formats](0002-storage-and-formats.md) | Accepted direction | Native originals, PostgreSQL application model, existing S3 storage, derived exports |
| [0003 Interfaces](0003-interfaces.md) | Accepted direction | First-class CLI, HTTP API/OpenAPI, reusable client SDK |
| [0004 Rust architecture](0004-rust-architecture.md) | Accepted for first build | Five-package workspace, async modular server, durable background jobs |
| [0005 Search](0005-search.md) | Proposed | Keyword baseline and optional profiled embeddings with exact initial vector search |
| [0006 Team access](0006-team-access.md) | Superseded by 0008 | Earlier project-bound access proposal; replaced by independent session sharing |
| [0007 Capability focused documentation](0007-capability-focused-documentation.md) | Accepted | Lead with storing, syncing, querying, and searching sessions; document shared access as a supporting capability |
| [0008 Collection and project associations](0008-collection-and-project-associations.md) | Accepted direction | Source-based collection, optional project labels, and sharing independent of classification |
| [0009 Reference storage and dependencies](0009-reference-storage-and-dependencies.md) | Accepted | Tested SeaweedFS/AWS-client pair and compiler/library baseline |
| [0010 Authentication](0010-reference-authentication.md) | Accepted | Tested Keycloak code/PKCE profile and explicit API audience |
| [0011 Normalized messages](0011-normalized-message-contract.md) | Accepted | Typed blocks, provider normalization, bounded previews, and native fixtures |
| [0012 First build](0012-first-build-scope.md) | Accepted | Frozen 23-operation storage-to-CLI slice |

An accepted direction does not mean every example field or default has been approved. Accept a proposed record after its assumptions and relevant open questions are reviewed. Supersede records with a new linked decision rather than erasing the rationale.
