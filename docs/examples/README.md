# Contract examples

These are synthetic examples of the SessionDB interface, including later target-v1 features. None contains a real credential or collected session. The server example describes accepted configuration fields; [development](../development.md) supplies a local runnable profile. See [implementation status](../implementation.md) for available API operations.

| File | Purpose |
| --- | --- |
| [server.toml](server.toml) | Server-only S3, authentication, telemetry, and max_file_bytes |
| [client.toml](client.toml) | Default server URL and selected native store roots |
| [source-create.json](source-create.json) | A native store source with no project binding |
| [session.json](session.json) | A home-launched session associated with a project elsewhere |
| [message.json](message.json) | Typed assistant text and JSON tool input |
| [content-reference.json](content-reference.json) | Conditional expansion of an oversized structured preview |
| [session-unassigned.json](session-unassigned.json) | A valid session with an empty project association array |
| [session-update.json](session-update.json) | Project association edits without an access change |
| [search-request.json](search-request.json) | A filtered shared keyword query |
| [search-response.json](search-response.json) | Bounded matches with retrieval and coverage information |
| [file-upload.json](file-upload.json) | JSON metadata multipart part, with explicit null expected_version |
| [file-upload-result.json](file-upload-result.json) | HTTP 200 per-file acceptance and parse diagnostics |
| [embedding-profile.json](embedding-profile.json) | A common embedding profile that also covers unassigned sessions |
| [problem.json](problem.json) | RFC 9457 error with a stable application code |

Synthetic IDs use `2222…` for project Payments, `5555…` for a source, `6666…` for a session, `7777…` for a message, and `8888…` for a source file. All domains/digests are illustrative.

Upload size/hash demonstrate field shape, not actual fixture bytes. A real request sends metadata plus raw JSONL and verifies both. Each successful file is accepted independently; no import/job examples or signed object URLs remain. Search, session updates, and embedding examples describe explicitly planned operations.

Each API example validates against its named schema in the [OpenAPI draft](../openapi.yaml). Configuration examples parse as TOML and retain the precedence/secret handling in [CLI](../cli.md) and [operations](../operations.md).
