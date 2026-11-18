# First build

Connect a client, collect a native session launched from any cwd, retain its original, and read/export it through the API and CLI. Projects and embeddings are optional.

Status: accepted sessions-first and synchronous per-file contract, [ADR 0014](decisions/0014-sessions-and-synchronous-files.md). Integrated redesign checks must be recorded after execution; historical results are not proof of this cutover.

## Selected stack

PostgreSQL catalog, S3-compatible immutable originals/artifacts through the official AWS client, and one required instance key. SeaweedFS 4.54 is the reference service. See [reference validation](validation/reference-stack.md) for the historical locked dependency baseline and [implementation status](implementation.md) for delivery boundaries.

## Baseline operation subset

[OpenAPI](openapi.yaml) marks 15 operations with `x-first-build: true`:

| Area | Operation IDs |
| --- | --- |
| Service | `getLiveness`, `getReadiness`, `getServerInfo` |
| Sources and originals | `listSources`, `createSource`, `getSource`, `listSourceFiles`, `getOriginalContent` |
| File upload | `uploadSourceFile` |
| Sessions | `listSessions`, `getSession`, `listSessionMessages`, `getMessage`, `getMessageContent`, `exportSession` |

Two artifact reads are additionally implemented, for 17 total. Native ZIP is the implemented export. Search, embeddings, project administration, changes, metadata mutations, and normalized exports remain planned. Unsupported planned routes return `501`; unsupported options return `400 unsupported_option` before mutation.

## Acceptance

- Collect a source with recorded cwd `/home/ada` and no project labels.
- Verify actual bytes and fully parse every record before registering/storing originals/artifacts; commit accepted catalog state before each HTTP 200.
- Reject unsupported/malformed native files before object registration, storage, or catalog mutation; retain only accepted valid originals.
- Retry an identical snapshot after disconnect without duplicate sessions/messages, including use of the previous expected-version token.
- Reject mismatched bytes and stale nonidentical versions without changing accepted state.
- Checkpoint each file separately; a second file failure never rolls back an earlier successful receipt.
- Verify complete-prefix appends, preserved rewrite generations, and manual metadata preservation.
- Keep typed content, branches, subagents, artifacts, ranges, and exact native ZIP bytes available.
- Reject missing/wrong keys everywhere under `/v1`; independent correct-key clients see the same collection.
- Recover client pending snapshots and catalog/original reads after outage, restart, and backup/restore.

Polling watch is implemented; notifications and OS service installation remain planned. The catalog is created directly in the initial migration: no deployment upgrade or legacy import/job protocol exists.
