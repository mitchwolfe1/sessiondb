# Collection and project associations

Status: accepted direction. Date: 2026-11-15.

## Decision

Collect sessions from configured native harness stores independently of their starting directories. Keep recorded cwd as metadata. A session may have zero or more optional project associations, with explicit assignment/reassignment and optional initial matching rules.

Keep project classification separate from sharing. A project label does not move the session into another access scope. The proposed access model uses organization membership, session visibility, and explicit user/team session grants. Sources belong to a device/account/harness store, not to one project.

## Rationale

A session can start in a home directory, work in a different repository, or touch several projects. A workspace containment requirement loses valid sessions and confuses native storage layout with subject matter. Logical associations support these cases without making collection depend on successful classification.

## Consequences

This replaces the project-bound collection and access proposal in [0006](0006-team-access.md) and updates the unreleased API draft to `0.2.0-draft`. The exact roles and authentication integration remain proposed in [access control](../access-control.md). Project filters and manual labels do not change session visibility; sync preserves those edits.

Embedding profiles operate independently of project assignment so unassigned sessions remain searchable. Original native bytes and references remain intact. Content-based project classification and project-derived permissions are outside this decision.
