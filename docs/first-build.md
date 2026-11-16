# First build

The first implementation delivers one complete path: connect a client, import a native session from a home-directory launch, persist it, and read/export it through the API and CLI. Project assignment and embeddings are not required for that path.

Status: implemented initial scope. See [implementation status](implementation.md) and [build instructions](development.md) for the working subset and remaining milestones.

## Selected stack

Use PostgreSQL for the catalog, S3-compatible object storage through the official AWS client, and OIDC Authorization Code/PKCE with a loopback CLI callback. SeaweedFS 4.54 and Keycloak 26.8.2 are local reference/test services. Existing compatible services can be configured through the same interfaces.

The selected dependency/toolchain baseline and initial compatibility results are in [reference stack validation](validation/reference-stack.md). The implementation covers the Codex, Claude Code, and Pi families represented by the native fixtures. Fixture conformance and the real storage-to-CLI integration path are checked by the test suite.

## Frozen operation subset

The [OpenAPI draft](openapi.yaml) marks the 23 selected operations with `x-first-build: true`.

| Area | Operation IDs |
| --- | --- |
| Service/login | `getLiveness`, `getReadiness`, `getServerInfo`, `getAuthConfig`, `registerLogin`, `getMe`, `listOrganizations` |
| Second-account setup | `createInvitation` |
| Collection source | `listSources`, `createSource`, `getSource`, `listSourceFiles`, `getOriginalContent` |
| Import | `createImport`, `getImport`, `completeImport`, `getJob` |
| Session access | `listSessions`, `getSession`, `listSessionMessages`, `getMessage`, `getMessageContent`, `exportSession` |

The first export is the native ZIP bundle. Two artifact reads are also implemented to expand inline images. Normalized/STS exports, metadata mutation, team/session grants, project administration, embeddings, and search follow their delivery milestones. A server advertises implemented capabilities; an unimplemented target-v1 route returns a documented `501` problem. An unsupported operation parameter returns `400 unsupported_option` before any mutation. Optional initial project labels that are unavailable still follow the import warning behavior and do not block capture.

## Implementation sequence

1. Create the Cargo workspace and commit the actual toolchain/lockfile for the selected dependencies. Add formatting, Clippy, and unit/integration checks.
2. Add migrations for the catalog, ownership, sources/files, imports/jobs, and normalized sessions/messages. Include optional project associations in the schema without requiring a project to import.
3. Wire the chosen S3 client and enforce durable acceptance after file verification. A stored object alone is not a finalized import.
4. Implement human login/token validation and first-owner bootstrap, then register the second account through an invitation. Reject missing/invalid API identity and cross-account private-session reads.
5. Add one-shot source sync and the Codex adapter against the native fixtures. Persist client acceptance checkpoints.
6. Implement session/message reads, content expansion, and native export through the shared SDK and CLI.
7. Exercise append, repeat completion, lost response, outage/restart, malformed complete lines, and incomplete tails.

Polling watch is implemented. Filesystem notifications and OS service installation follow separately. Keyword search is the next retrieval milestone; optional providers use the documented extension boundary.

## First-build acceptance

- An enabled source can collect a session with cwd `/home/ada` and `project_ids: []`.
- Uploaded bytes survive service restart and round-trip through authorized native export.
- Repeating an import/completion does not duplicate messages or sessions.
- A later append becomes readable without clearing catalog metadata or changing readers.
- A second member cannot read the owner's private session, while an explicitly organization-visible source/session follows that visibility.
- The CLI and SDK see the same session, ordering, errors, and processing state.
- Larger typed content expands through the full-content endpoint without invalid JSON truncation.

This path now runs against the selected contracts. Later performance tuning, retrieval features, and release publication remain in the delivery plan.
