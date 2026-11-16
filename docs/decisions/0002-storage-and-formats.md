# Original files and application storage

Status: accepted direction. Date: 2026-11-15.

## Decision

Preserve native session files and selected supporting artifacts. Maintain a consistent application model and search index separately. Use PostgreSQL for application data and an existing S3-compatible object service for originals. Standardized trace formats are exports/integrations rather than a required destructive conversion of collected data.

## Rationale

Native formats contain provider-specific branches, compaction, context edits, and inherited references. Keeping originals allows parser improvements while giving users normal session-level queries. PostgreSQL and S3 interfaces give the project established operating and integration patterns.

## Consequences

The chosen object client/provider must prove required compatibility. Object durability precedes catalog acceptance. Original ranges refer to uncompressed bytes. Storage details and retention policy require review, but a parser failure must not discard an accepted original.

SessionDB does not implement an S3 server, a custom object transfer language, or a new distributed storage engine. See [sync](../sync.md) and [operations](../operations.md).
