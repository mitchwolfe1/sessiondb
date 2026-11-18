# Design decisions

Decision records explain choices that affect several SessionDB components. Requirements accepted in the product conversation are distinct from implementation proposals awaiting review.

| Record | Status | Decision |
| --- | --- | --- |
| [0001 Product direction](0001-product-and-rust.md) | Accepted; identity assumptions superseded by 0013 | Session database and documentation-first development; implementation language recorded in 0001 |
| [0002 Storage and formats](0002-storage-and-formats.md) | Accepted direction | Native originals, PostgreSQL application model, existing S3 storage, derived exports |
| [0003 Interfaces](0003-interfaces.md) | Accepted direction | First-class CLI, HTTP API/OpenAPI, reusable client SDK |
| [0004 Rust architecture](0004-rust-architecture.md) | Accepted workspace; execution superseded by 0014, identity by 0013 | Five-package modular Rust server |
| [0005 Search](0005-search.md) | Proposed retrieval; workflow superseded by 0014, identity by 0013 | Keyword baseline and optional immutable embedding profiles |
| [0006 Team access](0006-team-access.md) | Superseded by 0008 and 0013 | Historical project-bound access proposal; no active identity policy |
| [0007 Capability focused documentation](0007-capability-focused-documentation.md) | Accepted; identity-feature assumptions superseded by 0013 | Lead with storing, syncing, querying, and searching sessions; document shared access as a supporting capability |
| [0008 Collection and project associations](0008-collection-and-project-associations.md) | Accepted direction; access assumptions superseded by 0013 | Source-based collection and optional project labels |
| [0009 Reference storage and dependencies](0009-reference-storage-and-dependencies.md) | Baseline accepted; staging/finalization superseded by 0014 | Historical SeaweedFS/AWS-client and toolchain checks |
| [0010 Authentication](0010-reference-authentication.md) | Superseded by 0013 | Historical Keycloak code/PKCE profile and API audience |
| [0011 Normalized messages](0011-normalized-message-contract.md) | Accepted; conditional reads superseded by 0015 | Typed blocks, provider normalization, bounded previews, and native fixtures |
| [0012 First build](0012-first-build-scope.md) | Counts/protocol superseded by 0013 and 0014 | Historical first slice; current 15 baseline plus two artifact reads |
| [0013 Instance API key](0013-instance-api-key.md) | Key boundary accepted; catalog/count/object URL assumptions superseded by 0014, transport by 0016 | One collection and required shared key |
| [0014 Sessions and synchronous files](0014-sessions-and-synchronous-files.md) | Accepted; permissive archival superseded by 0015 | Eleven domain tables, API-mediated per-file acceptance/recovery, no batch/job machinery |
| [0015 Strict native records and simple reads](0015-strict-native-records-and-simple-reads.md) | Accepted | Whole-file rejection before storage; no read revisions or parser diagnostics |
| [0016 HTTP on private networks](0016-http-private-networks.md) | Accepted | HTTP and HTTPS URLs; operator-managed network protection with the same required API key |

An accepted direction does not mean every example field or default has been approved. Accept a proposed record after its assumptions and relevant open questions are reviewed. Supersede records with a new linked decision rather than erasing the rationale.
