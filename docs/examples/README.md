# Contract examples

These are synthetic documentation examples of the proposed SessionDB interface. They are not executable application configuration today, and none contains a real credential or collected session.

| File | Purpose |
| --- | --- |
| [server.toml](server.toml) | Single-server storage, authentication, worker, and limit settings |
| [client.toml](client.toml) | Native store roots and an optional initial classification rule |
| [source-create.json](source-create.json) | A native store source with no project binding |
| [session.json](session.json) | A home-launched session associated with a project elsewhere |
| [message.json](message.json) | Typed assistant text and JSON tool input |
| [content-reference.json](content-reference.json) | Conditional expansion of an oversized structured preview |
| [session-unassigned.json](session-unassigned.json) | A valid session with an empty project association array |
| [session-update.json](session-update.json) | Project association edits without a sharing change |
| [search-request.json](search-request.json) | A filtered shared keyword query |
| [search-response.json](search-response.json) | Bounded matches with retrieval and coverage information |
| [import-request.json](import-request.json) | Source file snapshot manifest |
| [import-response.json](import-response.json) | Signed upload target returned before durable acceptance |
| [import-accepted.json](import-accepted.json) | Post-verification receipt that permits a client checkpoint |
| [embedding-profile.json](embedding-profile.json) | A common embedding profile that also covers unassigned sessions |
| [problem.json](problem.json) | RFC 9457 error with a stable application code |
| [keycloak-realm.json](keycloak-realm.json) | Reference public client/scopes/audience mapper, without user credentials |

The synthetic IDs consistently use `1111…` for organization Acme, `2222…` for project Payments, `4444…` for engineer Ada, `5555…` for a source, `6666…` for a session, `7777…` for a message, and `8888…` for an import. Paths under `example.com` and all digest/URL values are illustrative.

Import sizes/digests demonstrate field shape, not an actual uploaded file. A real completion verifies the actual object bytes and rejects a mismatch. Signed URLs are returned by the service and expire; clients never construct or persist them as permanent citations.

Each API example validates against its named schema in the [OpenAPI draft](../openapi.yaml). Configuration examples parse as TOML and retain the precedence/secret handling in [CLI](../cli.md) and [operations](../operations.md).
