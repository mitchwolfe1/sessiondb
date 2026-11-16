# Open questions

These decisions guide upcoming implementation work. Rust, Codex/Claude/Pi support, automatic sync, conventional interfaces, and extensible search define the project direction. Shared access is specified alongside the other server capabilities.

| ID | Decision | Concrete proposal and alternatives | Blocks |
| --- | --- | --- | --- |
| Q01 | Rust dependency set and MSRV | Use the workspace/library candidates in architecture; prove a compatible dependency set and lock one tested stable toolchain. | Cargo workspace and release packaging |
| Q02 | Identity provider and CLI interoperability | OIDC Authorization Code/PKCE with loopback login and exact issuer/subject bootstrap; test against the chosen provider. Device authorization can be added if required. | Authentication and first multi-user slice |
| Q03 | S3 implementation/client | Choose a deployed compatible service and Rust client after testing presigned uploads, checksum validation, conditional operations, and endpoint reachability. | Durable import implementation |
| Q04 | Accept the OpenAPI draft | Review the checked-in OpenAPI 3.1 schemas, examples, and CLI mapping. Confirm optional fields and mutation semantics before implementing them. | HTTP routes and generated bindings |
| Q05 | Retention and limits | No automatic expiry by default; coalesce append snapshots, preserve rewrites, and require explicit deletion. Review practical organization limits and backup retention. | Production collection and cleanup |
| Q06 | First embedding profile and provider contract | Configured OpenAI-compatible endpoint, separate query/document transforms, exact cosine retrieval; verify dimensions and supported response/usage fields. | Semantic-search milestone |
| Q07 | Binary distribution and SDK publishing | Linux/macOS release archives and user-service installation; Rust SDK first. Decide package registries, signing, and license before publication. | V1 release |
| Q08 | Native compatibility matrix | Build synthetic fixtures from supported Codex/Claude/Pi shapes, including forks, subagents, and artifacts; record tested versions. | All-harness acceptance |
| Q09 | Conversation leaf and export details | History is default; optional leaf ancestry; native bundle preserves originals. Review cyclic/missing-reference display and STS projection warnings. | Detailed message/export implementation |
| Q10 | Search ranking and capacity objectives | Keyword default, explicit semantic/hybrid, RRF proposal; select candidate limits and benchmark with permission filtering and current model dimensions. | Search performance/release claims |

The maintainer resolves a question by accepting or replacing a decision record and updating every affected feature document/example. There is no need to settle later-stage questions before making the current documentation coherent. Do not implement a blocked surface against an undocumented assumption.
