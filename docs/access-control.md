# Access control

SessionDB is an organization service from the beginning. Engineers publish work into configured projects, and project access governs every representation of those sessions.

Status: proposed v1 role and authentication contract. Identity-provider interoperability and bootstrap are review gates before implementation.

## Roles

Organization roles are `owner`, `admin`, and `member`. Owners control organization settings and owner membership. Admins manage teams, projects, non-owner memberships, grants, sources, and jobs. Members receive access through direct or team project grants. The last active owner cannot be removed or demoted.

Project roles are ordered `viewer`, `contributor`, and `maintainer`. The strongest applicable direct/team grant wins. Organization owners and admins have maintainer access to every project in their organization; this is an explicit company-managed workspace policy.

| Action | Viewer | Contributor | Maintainer | Organization admin or owner |
| --- | --- | --- | --- | --- |
| List, read, search, export project sessions | Yes | Yes | Yes | Yes |
| Register own source and upload sessions | No | Yes | Yes | Yes |
| Update own session title/tags | No | Yes | Yes | Yes |
| Update any project session title/tags | No | No | Yes | Yes |
| Pause/revoke another member's project source | No | No | Yes | Yes |
| Delete project sessions or request reindex | No | No | Yes | Yes |
| Manage project access grants | No | No | Yes | Yes |
| Create projects or manage teams/members | No | No | No | Yes |
| Configure embedding provider credentials | No | No | No | Yes |
| Grant owner role or remove an owner | No | No | No | Owner only |

An engineer can pause their own collection source. A source token has narrower project/source permissions than an interactive account and cannot inherit organization administration implicitly.

## Membership

An account is identified by a validated identity-provider issuer and subject. An administrator invites an email through the configured provider; a `POST /v1/auth/session` login with a matching verified email claims the pending invitation and becomes a membership. Store the subject at acceptance. Read-only `GET /v1/me` does not create memberships. Removing a membership immediately removes its team participation and effective access, and disables collection credentials issued for that membership.

V1 has no private user workspace inside a company organization. Personal work belongs in a separate organization or outside configured collection roots. The client shows the destination project before enabling a mapping.

## Authentication proposal

Use a configured OpenID Connect provider for human authentication and expiring, revocable project-scoped API tokens for applications and automation. The Rust CLI uses the external browser with Authorization Code and PKCE and a loopback redirect. The provider must support a registered public client and an access-token audience for SessionDB. Validate access tokens; do not accept an ID token as an API bearer token merely because it is signed.

The server publishes public authentication configuration at `/v1/auth/config`. The CLI stores credentials in the operating-system credential store, or reads a scoped token from `SESSIONDB_TOKEN`/standard input for headless use. No client secret belongs in a distributed CLI binary.

For first startup, the operator configures the initial organization and an exact trusted issuer/subject to become its first owner. The initialization transaction runs only against an empty installation. Later changes to this bootstrap configuration do not grant new owner privileges. See [open questions](open-questions.md) for the interoperability test required before accepting this proposal.

An interactive member can issue a project token up to their current project rights. `sessions:read` permits project reading/search/export, `sessions:write` permits allowed session metadata/deletion operations, and `sources:sync` permits collection for one bound own source. Collection-only tokens can inspect their own sync state but cannot read arbitrary conversations. A token never grants organization administration, and its rights are rechecked against the issuing membership's current project access. Dedicated service-account provisioning is later work; v1 automation uses a member-issued scoped token.

## Authorization enforcement

The authenticated principal's organization membership and project grants are checked on list, detail, content, artifact, export, changes, and search routes. Apply filters before lexical/vector ranking and pagination. A session ID or native path supplied by a client never bypasses membership checks.

For an authenticated caller, inaccessible project/session resources return `404` to avoid revealing their existence. Known resources with a denied management action return `403`. Unauthenticated requests return `401` with the standard authentication challenge. Rate limiting uses `429` and `Retry-After`.

Background jobs carry their organization/project scope. Job result endpoints recheck the caller's current permissions. Revoking a grant changes access immediately; caches must not authorize from stale search or embedding results.

Use short-lived object download URLs only after authorization. Previously issued URLs may remain usable until their expiry; the proposed maximum is 60 seconds. If immediate revocation is required, proxy downloads through SessionDB instead. This deployment policy also applies to exports.

## Collection and content handling

Collect only configured workspaces, selected sessions, and allowlisted supporting artifacts. Never recursively upload a harness home containing credentials and unrelated configuration. Transcript text is data: ingestion never executes recorded commands or follows arbitrary remote links found inside content.

Original files can contain sensitive content already recorded by the harness. Use authenticated private storage and project permissions. Any client-side redaction mode must explicitly identify its output as a transformed copy; it must not claim byte-identical archival preservation. A mandatory redaction policy is a deployment/product decision, not an undocumented parser side effect.

## Required access scenarios

- A member in team A can search and export a granted project, but cannot discover team B's ungranted project through results, counts, jobs, or object links.
- A source token can sync only its bound source and cannot grant itself access.
- Membership removal stops new imports and interactive reads while already accepted team history remains owned by the project.
- A revoked grant affects keyword search, semantic search, raw downloads, and queued exports consistently.
- Administrator and owner capabilities follow the role table, including the last-owner constraint.
