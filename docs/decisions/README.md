# Design decisions

Decision records explain choices that affect several SessionDB components. Requirements accepted in the product conversation are distinct from implementation proposals awaiting review.

| Record | Status | Decision |
| --- | --- | --- |
| [0001 Product and Rust](0001-product-and-rust.md) | Accepted | Session database, Rust implementation, documentation first; framing clarified by 0007 |
| [0002 Storage and formats](0002-storage-and-formats.md) | Accepted direction | Native originals, PostgreSQL application model, existing S3 storage, derived exports |
| [0003 Interfaces](0003-interfaces.md) | Accepted direction | First-class CLI, HTTP API/OpenAPI, reusable Rust SDK |
| [0004 Rust architecture](0004-rust-architecture.md) | Proposed | Five-package workspace, async modular server, durable background jobs |
| [0005 Search](0005-search.md) | Proposed | Keyword baseline and optional profiled embeddings with exact initial vector search |
| [0006 Team access](0006-team-access.md) | Superseded by 0008 | Earlier project-bound access proposal; replaced by independent session sharing |
| [0007 Capability focused documentation](0007-capability-focused-documentation.md) | Accepted | Lead with storing, syncing, querying, and searching sessions; document shared access as a supporting capability |
| [0008 Collection and project associations](0008-collection-and-project-associations.md) | Accepted direction | Source-based collection, optional project labels, and sharing independent of classification |

An accepted direction does not mean every example field or default has been approved. Accept a proposed record after its assumptions and relevant open questions are reviewed. Supersede records with a new linked decision rather than erasing the rationale.
