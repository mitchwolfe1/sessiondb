# Access control

Session access is independent of project labels and directory matching. A session's owner, visibility, and explicit grants control reading and editing. Sources remain tied to the account and native store they collect.

Status: proposed role/authentication contract, replacing the earlier project-bound proposal through [0008](decisions/0008-collection-and-project-associations.md).

## Session access

New sources default to `private` visibility. A session with private visibility is readable by its owner, organization owners/admins, and explicitly granted users/teams. `organization` visibility additionally allows all active organization members to read it.

A viewer grant permits reading, searching, and exporting. An editor grant also permits changing title, tags, and project associations. Only the session owner or an organization administrator can change visibility/grants or delete the session. Linking a parent/child session does not grant access to the linked session.

| Action | Reader | Editor | Session owner | Organization admin or owner |
| --- | --- | --- | --- | --- |
| Read, search, export accessible sessions | Yes | Yes | Yes | Yes |
| Update title, tags, project associations | No | Yes | Yes | Yes |
| Change visibility or user/team grants | No | No | Yes | Yes |
| Delete a session | No | No | Yes | Yes |
| Register a source | Active member | Active member | Active member | Yes |
| Pause a source or read its unparsed originals | No | No | Source owner | Yes |
| Configure embedding providers or organization-wide indexing | No | No | No | Yes |

Every active member can create a logical project label. Its creator or an administrator can change/archive it. Project labels are organization metadata; viewing a project's name does not authorize reading any of its sessions. Project archive has no effect on native collection.

## Membership and teams

Organization roles are `owner`, `admin`, and `member`. Owners manage organization settings and owner membership. Admins manage teams and non-owner memberships and can manage/read all sessions in their organization. The last active owner cannot be removed or demoted.

An account is identified by a validated issuer/subject. An administrator invites an email, and a `POST /v1/auth/session` login with a matching verified email claims the invitation. Read-only `GET /v1/me` creates no memberships. Removing membership invalidates its grants and credentials; already accepted data remains stored until explicitly deleted.

Teams group memberships for explicit session grants. Project association is never an implicit grant to a team. Private and shared sessions can coexist in the same organization and reference the same project label.

## Sources and sync

A source belongs to an account, device, harness, and native store root. It has no project binding. Its default visibility applies only when a new session is created; later sync preserves per-session access and classification edits.

The user selects which native stores to collect and can add include/exclude filters. A starting directory outside a repository is not an exclusion by itself. Supporting-file allowlists avoid collecting credentials/configuration simply because they sit beside transcripts.

Raw source listing and unparsed originals require source-owner/admin access. A session reader's native export includes only fully authorized files and artifacts. A file containing another inaccessible session is omitted; unrelated paths/content are not leaked through the warning.

## Authentication and tokens

Use a configured OIDC provider for human login and expiring revocable API tokens for automation. The CLI proposal uses an external browser with Authorization Code/PKCE and a loopback redirect. The provider needs a registered public client and the SessionDB API audience. Validate access tokens rather than accepting an ID token as API authorization.

The server exposes public login configuration at `/v1/auth/config`. The CLI uses the OS credential store or explicit token input/environment. Empty-installation bootstrap configures one exact trusted issuer/subject as the initial owner; changing bootstrap settings later grants no new privileges.

A member-issued token can carry `sessions:read`, `sessions:write`, or `sources:sync`. Read/write rights are limited by the issuer's current session access and optional explicit `session_ids` restrictions. A sync token also binds one owned source and can inspect its own import/file state; a sync-only token cannot change source defaults or sharing. Source configuration uses the interactive owner/admin identity. Project filters are not token permission scopes. Tokens never grant organization administration.

## Enforcement

Apply organization membership and session authorization before list pagination, snippets, counts, keyword/vector retrieval, changes, exports, and artifact downloads. Jobs recheck the current caller before returning results. Indexes and result caches cannot authorize from stale grants.

Use `401` for unauthenticated requests, `404` for inaccessible data, and `403` for a denied known management operation. Previously issued signed URLs can remain usable for their documented maximum lifetime of 60 seconds; proxy through SessionDB if immediate download revocation is required.

Original transcripts can contain sensitive recorded content. Storage is private and authenticated. Ingestion treats text as data and never executes recorded commands or follows arbitrary remote content URLs. Any redaction mode explicitly identifies its output as a transformed copy.

## Access scenarios

- An unassigned private session is collected from a home-directory launch and is readable by its owner.
- Assigning/removing project labels leaves its readers unchanged.
- A session granted to a team appears in that team's authorized queries, including keyword and semantic search.
- An editor can reclassify a session but cannot change sharing or delete it.
- Revocation/membership removal affects all read paths and invalidates applicable tokens; new sync cannot reset the revoked grant.
- A source token updates only its source and cannot grant itself session-read or organization rights.
